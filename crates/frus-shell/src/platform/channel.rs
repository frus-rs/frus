// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Values from a platform callback to the interface's thread (milestone 839).
//!
//! A listener the system calls runs on a thread of the system's: a Java binder or looper
//! thread on Android, a sensor's, a network callback's. What it learns must reach the
//! interface's thread, where the state lives. [`Sender::send`] queues the value and wakes the
//! task awaiting [`Receiver::recv`]; that task is an ordinary task of the interface's thread
//! ([`spawn_local`](frus_widgets::spawn_local), [`StateHandle::spawn`](frus_widgets::StateHandle::spawn)),
//! so waking it asks the window for a frame and the frame runs it: the same path every task
//! woken from another thread takes, with no busy loop and nothing done while nothing is sent.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

struct Inner<T> {
    queue: VecDeque<T>,
    /// The task waiting in [`Receiver::recv`], woken by the next value or the last sender.
    waiter: Option<Waker>,
    senders: usize,
    receiver: bool,
}

type Shared<T> = Arc<Mutex<Inner<T>>>;

fn lock<T>(shared: &Shared<T>) -> std::sync::MutexGuard<'_, Inner<T>> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// A channel from any thread to a task of the interface's thread: the way a platform
/// callback hands what it learned to the application.
///
/// The [`Sender`] is `Send` and `Clone`: it goes into the listener, which may be called on
/// any thread. The [`Receiver`] stays on the interface's thread, in a task that may hold the
/// state it changes:
///
/// ```no_run
/// use frus_shell::platform;
///
/// let (tx, mut rx) = platform::channel::<u32>();
/// std::thread::spawn(move || {
///     // A system callback, on a thread of its own.
///     tx.send(42);
/// });
/// frus_widgets::spawn_local(async move {
///     while let Some(value) = rx.recv().await {
///         // On the interface's thread: change a state, show it.
///         let _ = value;
///     }
/// });
/// ```
///
/// Values are kept in order and none is dropped while the receiver lives. Nothing runs and
/// no frame is asked for until a value is sent.
pub fn channel<T: Send + 'static>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Mutex::new(Inner {
        queue: VecDeque::new(),
        waiter: None,
        senders: 1,
        receiver: true,
    }));
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}

/// The sending half of a [`channel`]: called from any thread.
pub struct Sender<T> {
    shared: Shared<T>,
}

impl<T> Sender<T> {
    /// Queues `value` for the receiver and wakes the task awaiting it. `false` when the
    /// receiver is gone, the value then being dropped: a listener can stop itself on it.
    pub fn send(&self, value: T) -> bool {
        let waiter = {
            let mut inner = lock(&self.shared);
            if !inner.receiver {
                return false;
            }
            inner.queue.push_back(value);
            inner.waiter.take()
        };
        // Woken outside the lock: the waker asks the window for a frame.
        if let Some(waiter) = waiter {
            waiter.wake();
        }
        true
    }

    /// Whether the receiver is still there to be sent to.
    pub fn is_open(&self) -> bool {
        lock(&self.shared).receiver
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        lock(&self.shared).senders += 1;
        Sender {
            shared: self.shared.clone(),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let waiter = {
            let mut inner = lock(&self.shared);
            inner.senders -= 1;
            if inner.senders == 0 {
                inner.waiter.take()
            } else {
                None
            }
        };
        // The last sender gone: the receiver's `recv` answers `None` once the queue is empty.
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }
}

impl<T> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender").finish_non_exhaustive()
    }
}

/// The receiving half of a [`channel`]: awaited by a task of the interface's thread.
pub struct Receiver<T> {
    shared: Shared<T>,
}

impl<T> Receiver<T> {
    /// The next value, in the order sent; `None` once every sender is gone and the queue is
    /// empty.
    pub fn recv(&mut self) -> impl Future<Output = Option<T>> + '_ {
        Recv { receiver: self }
    }

    /// The next value if one is queued, without waiting.
    pub fn try_recv(&mut self) -> Option<T> {
        lock(&self.shared).queue.pop_front()
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let mut inner = lock(&self.shared);
        inner.receiver = false;
        inner.queue.clear();
        inner.waiter = None;
    }
}

impl<T> std::fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receiver").finish_non_exhaustive()
    }
}

struct Recv<'a, T> {
    receiver: &'a mut Receiver<T>,
}

impl<T> Future for Recv<'_, T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let mut inner = lock(&self.receiver.shared);
        if let Some(value) = inner.queue.pop_front() {
            return Poll::Ready(Some(value));
        }
        if inner.senders == 0 {
            return Poll::Ready(None);
        }
        inner.waiter = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;

    struct Count(AtomicUsize);

    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn poll<T>(rx: &mut Receiver<T>, waker: &Waker) -> Poll<Option<T>> {
        let mut future = std::pin::pin!(rx.recv());
        future.as_mut().poll(&mut Context::from_waker(waker))
    }

    /// A value sent from another thread wakes the waiting task once, and arrives in order.
    #[test]
    fn a_value_sent_from_another_thread_wakes_the_receiver() {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let (tx, mut rx) = channel::<u32>();
        assert!(poll(&mut rx, &waker).is_pending());
        assert_eq!(
            count.0.load(Ordering::SeqCst),
            0,
            "nothing sent, nothing woken"
        );
        std::thread::spawn(move || {
            assert!(tx.send(1));
            assert!(tx.send(2));
        })
        .join()
        .unwrap();
        assert_eq!(
            count.0.load(Ordering::SeqCst),
            1,
            "one wake for the waiting task"
        );
        assert_eq!(poll(&mut rx, &waker), Poll::Ready(Some(1)));
        assert_eq!(poll(&mut rx, &waker), Poll::Ready(Some(2)));
        // The sender went with its thread: the queue drained, the channel ends.
        assert_eq!(poll(&mut rx, &waker), Poll::Ready(None));
    }

    /// The last sender dropping wakes a waiting receiver, which then ends.
    #[test]
    fn the_last_sender_gone_ends_the_receiver() {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let (tx, mut rx) = channel::<u32>();
        let other = tx.clone();
        assert!(poll(&mut rx, &waker).is_pending());
        drop(tx);
        assert_eq!(count.0.load(Ordering::SeqCst), 0, "one sender is left");
        drop(other);
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
        assert_eq!(poll(&mut rx, &waker), Poll::Ready(None));
    }

    /// With the receiver gone, a send says so and keeps nothing.
    #[test]
    fn a_send_without_a_receiver_says_so() {
        let (tx, rx) = channel::<String>();
        assert!(tx.is_open());
        drop(rx);
        assert!(!tx.is_open());
        assert!(!tx.send("lost".into()));
    }

    /// A value sent from another thread runs the task awaiting it on the interface's thread,
    /// through the widget layer's wake: the wake asks for a frame, the frame runs the task.
    #[test]
    fn a_send_wakes_a_task_of_the_interface_thread() {
        let frames = Arc::new(AtomicUsize::new(0));
        let asked = frames.clone();
        frus_widgets::set_local_wake(move || {
            asked.fetch_add(1, Ordering::SeqCst);
        });
        let (tx, mut rx) = channel::<u32>();
        let got = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = got.clone();
        frus_widgets::spawn_local(async move {
            while let Some(value) = rx.recv().await {
                seen.set(seen.get() + value);
            }
        });
        while frus_widgets::run_local_tasks() {}
        assert_eq!(got.get(), 0);
        let before = frames.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            tx.send(5);
        })
        .join()
        .unwrap();
        assert!(
            frames.load(Ordering::SeqCst) > before,
            "the send asked for a frame"
        );
        while frus_widgets::run_local_tasks() {}
        assert_eq!(got.get(), 5, "the frame ran the task, on this thread");
    }
}
