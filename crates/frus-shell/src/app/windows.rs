// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **More windows than one** (milestone 827, #218): the windows the application lists, opened
//! and closed to follow its state, and each window's events and frames run against that
//! window's own state.
//!
//! Every input and frame method of the shell reads `self.win`, the window in front of it. The
//! others wait in `parked`; an event for one of them swaps its state in (two pointers move),
//! and the work every window has at each turn of the loop — its timers, its accessibility
//! actions, the wait it asks for — is done for each in turn by [`App::each_window`]. An
//! application with one window never swaps anything.

use super::*;

/// The sooner of two waits: the loop wakes for whichever window needs it first.
pub(super) fn sooner(current: Option<ControlFlow>, wanted: ControlFlow) -> ControlFlow {
    match (current, wanted) {
        (None, wanted) => wanted,
        (Some(ControlFlow::Poll), _) | (_, ControlFlow::Poll) => ControlFlow::Poll,
        (Some(ControlFlow::WaitUntil(a)), ControlFlow::WaitUntil(b)) => {
            ControlFlow::WaitUntil(a.min(b))
        }
        (Some(ControlFlow::WaitUntil(at)), ControlFlow::Wait)
        | (Some(ControlFlow::Wait), ControlFlow::WaitUntil(at)) => ControlFlow::WaitUntil(at),
        (Some(ControlFlow::Wait), ControlFlow::Wait) => ControlFlow::Wait,
    }
}

impl<A: Application> App<A> {
    /// Brings the window `id` in front of the shell's code: whether it is one of this
    /// application's. An event still on its way from a window just closed is not.
    pub(super) fn activate(&mut self, id: WindowId) -> bool {
        // No window yet (a test's shell), or the one in front.
        if self.win.window.as_ref().is_none_or(|w| w.id() == id) {
            return true;
        }
        let Some(at) = self.parked.iter().position(|(parked, _)| *parked == id) else {
            return false;
        };
        self.swap_in(at);
        true
    }

    /// Brings the main window in front: what belongs to it alone — where it is kept, the
    /// title bar it shares — is read from there.
    pub(super) fn activate_main(&mut self) {
        if self.win.id.is_none() {
            return;
        }
        if let Some(at) = self.parked.iter().position(|(_, w)| w.id.is_none()) {
            self.swap_in(at);
        }
    }

    /// Exchanges the window in front with the one parked at `at`.
    fn swap_in(&mut self, at: usize) {
        std::mem::swap(&mut self.win, &mut self.parked[at].1);
        self.parked[at].0 = window_id_of(&self.parked[at].1);
    }

    /// Runs `f` once for each window, that window in front each time, and puts back the one
    /// that was in front. `f` must not open or close windows.
    pub(super) fn each_window(&mut self, mut f: impl FnMut(&mut Self)) {
        f(self);
        for at in 0..self.parked.len() {
            std::mem::swap(&mut self.win, &mut self.parked[at].1);
            f(self);
            std::mem::swap(&mut self.win, &mut self.parked[at].1);
        }
    }

    /// The main window, wherever its state is: the tray, the taskbar button and the global
    /// shortcuts belong to it.
    #[cfg_attr(not(desktop), allow(dead_code))]
    pub(super) fn main_window(&self) -> Option<Arc<Window>> {
        if self.win.id.is_none() {
            return self.win.window.clone();
        }
        self.parked
            .iter()
            .find(|(_, w)| w.id.is_none())
            .and_then(|(_, w)| w.window.clone())
    }

    /// Every other window is to be built again and drawn: the application's state changed
    /// (`tree`), or something its components keep did (`components`).
    pub(super) fn mark_other_windows(&mut self, tree: bool, components: bool) {
        for (_, other) in &mut self.parked {
            other.build_dirty |= tree;
            other.components_dirty |= components;
            if let Some(window) = &other.window {
                window.request_redraw();
            }
        }
    }

    /// After a window was built: the timers its components asked for are its own, and every
    /// window's are what the application's subscription reads (milestone 827). A build keeps
    /// only its own window's.
    pub(super) fn share_intervals(&mut self) {
        if self.parked.is_empty() {
            return;
        }
        self.win.intervals = frus_widgets::intervals();
        self.publish_intervals();
    }

    /// Hands every window's timers to the subscription.
    fn publish_intervals(&self) {
        let all = self
            .win
            .intervals
            .iter()
            .chain(self.parked.iter().flat_map(|(_, w)| w.intervals.iter()))
            .cloned()
            .collect();
        frus_widgets::__set_intervals(all);
    }

    /// What one window has to do as the loop runs out of events, and the wait it asks for, if
    /// it asks for one.
    pub(super) fn window_about_to_wait(&mut self) -> Option<ControlFlow> {
        // Where the window is now that its events are all in (milestone 783).
        #[cfg(desktop)]
        self.note_window();
        let mut wants = self.decide_hold();
        // A frame of decoration is owed: its deadline is set by the frame just drawn, after
        // the turn's start chose how long to wait, so the wait is chosen again (milestone 783).
        wants |= self.win.decoration_due.is_some();
        if self.win.file_drag.is_some() {
            self.deliver_dropped_files();
            if self.win.file_drag.is_some() {
                let at = self.system_cursor();
                self.files_moved(at);
            }
            wants = true;
        }
        wants.then(|| self.idle_control_flow())
    }

    /// What one window has to do as the loop wakes: `timer`, when it woke because a wait
    /// asked for was over.
    pub(super) fn window_new_events(&mut self, timer: bool) {
        if timer {
            self.poll_pending_tap(Instant::now());
        }
        // The caret's turn is due: a frame to show it, or to hide it, in (milestone 513).
        if timer
            && self.lifecycle == Lifecycle::Resumed
            && self.win.caret.next_toggle(Instant::now()).is_some()
        {
            self.request_redraw();
        }
        // Decoration's next frame is due (milestone 783).
        if self
            .win
            .decoration_due
            .is_some_and(|due| Instant::now() + DECORATION_SLACK >= due)
        {
            self.win.decoration_due = None;
            self.request_redraw();
        }
        // Actions an assistive technology asked for, through AccessKit.
        #[cfg(desktop)]
        self.drain_a11y_actions();
        // Actions a screen reader asked for, through the DOM bridge (#18).
        #[cfg(web)]
        self.drain_a11y_actions();
        // And what the views hosted over the surface sent (milestone 802).
        #[cfg(any(windows, web))]
        self.drain_hosted_views();
        // And what the browser did to the field it edits for the application (milestone 711).
        #[cfg(web)]
        self.drain_web_editing();
        // The regions the browser scrolled to show what a find found (milestone 830).
        #[cfg(web)]
        self.drain_web_text_scrolls();
    }

    /// Brings the windows open to what the application lists (milestone 827): opens, closes
    /// and retitles. Desktops only; once the main window has its renderer, which the others
    /// share.
    #[cfg(desktop)]
    pub(super) fn sync_windows(&mut self, event_loop: &ActiveEventLoop) {
        let wanted = self.app.windows();
        if wanted.is_empty() && self.parked.is_empty() {
            return;
        }
        if self.renderer.is_none() {
            return;
        }
        let fallback = self.app.title();
        let open: Vec<(String, String)> = std::iter::once(&self.win)
            .chain(self.parked.iter().map(|(_, w)| w))
            .filter_map(|w| Some((w.id.clone()?, w.title.clone())))
            .collect();
        let shown: Vec<(&str, &str)> = open
            .iter()
            .map(|(id, title)| (id.as_str(), title.as_str()))
            .collect();
        let changes = crate::window::window_changes(&shown, &wanted, &fallback);
        for id in &changes.close {
            self.close_window(id);
        }
        for (id, title) in changes.retitle {
            if let Some(state) = self.window_state(&id) {
                if let Some(window) = &state.window {
                    window.set_title(&title);
                }
                state.title = title;
            }
        }
        for at in changes.open {
            self.open_window(event_loop, &wanted[at]);
        }
        // What the close button sends follows the state too.
        for listed in &wanted {
            if let Some(state) = self.window_state(listed.id()) {
                state.on_close = Some(listed.close_message().clone());
            }
        }
    }

    /// The state of the window the application calls `id`, in front or parked.
    #[cfg(desktop)]
    fn window_state(&mut self, id: &str) -> Option<&mut WindowState<A>> {
        if self.win.id.as_deref() == Some(id) {
            return Some(&mut self.win);
        }
        self.parked
            .iter_mut()
            .map(|(_, w)| &mut **w)
            .find(|w| w.id.as_deref() == Some(id))
    }

    /// Opens the window `listed` asks for: hidden until its accessibility adapter is there, as
    /// the main window, then shown and asked for its first frame.
    #[cfg(desktop)]
    fn open_window(&mut self, event_loop: &ActiveEventLoop, listed: &crate::Window<A::Message>) {
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        let title = listed
            .title_text()
            .map(str::to_string)
            .unwrap_or_else(|| self.app.title());
        let (min_w, min_h) = listed.smallest_size().unwrap_or(MIN_WINDOW);
        let mut attributes = Window::default_attributes()
            .with_title(&title)
            .with_visible(false)
            .with_min_inner_size(winit::dpi::LogicalSize::new(min_w, min_h));
        if let Some((w, h)) = listed.opening_size() {
            attributes = attributes.with_inner_size(winit::dpi::LogicalSize::new(w, h));
        }
        let icon = crate::icon::window_icon(&self.app.icon());
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes = attributes.with_taskbar_icon(icon.clone());
        }
        attributes = attributes.with_window_icon(icon);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                log::error!("the window \"{}\" could not be made: {err}", listed.id());
                return;
            }
        };
        let size = window.inner_size();
        let surface =
            match renderer.add_surface(window.clone(), size.width.max(1), size.height.max(1)) {
                Ok(surface) => surface,
                Err(err) => {
                    log::error!(
                        "the window \"{}\" could not be drawn in: {err:#}",
                        listed.id()
                    );
                    return;
                }
            };
        let mut state = Box::new(WindowState::new(Some(listed.id().to_string())));
        state.title = title;
        state.on_close = Some(listed.close_message().clone());
        state.scale = window.scale_factor() as f32;
        state.surface = Some(surface);
        state.a11y = Some(crate::a11y::A11y::new(event_loop, &window));
        window.set_visible(true);
        window.request_redraw();
        state.window = Some(window.clone());
        // The timers the only window asked for were the subscription's whole; from now on
        // each window's are kept apart.
        if self.parked.is_empty() {
            self.win.intervals = frus_widgets::intervals();
        }
        self.parked.push((window.id(), state));
        log::info!("window \"{}\" opened", listed.id());
    }

    /// Closes the window the application calls `id`: its surface, its interface and what its
    /// components kept go with it, and so do the timers they asked for.
    #[cfg(desktop)]
    pub(super) fn close_window(&mut self, id: &str) {
        if self.win.id.as_deref() == Some(id) {
            self.activate_main();
        }
        let Some(at) = self
            .parked
            .iter()
            .position(|(_, w)| w.id.as_deref() == Some(id))
        else {
            return;
        };
        let (_, state) = self.parked.remove(at);
        // The surface goes before its window: the fields are dropped in that order.
        drop(state);
        self.publish_intervals();
        self.sync_subscriptions();
        log::info!("window \"{id}\" closed");
    }

    /// Makes each other window's surface again on a new renderer, after the GPU device was
    /// lost: the old ones belonged to the old device.
    #[cfg_attr(web, allow(dead_code))]
    pub(super) fn remake_surfaces(&mut self) {
        let Some(renderer) = self.renderer.as_ref() else {
            return;
        };
        for state in std::iter::once(&mut self.win).chain(self.parked.iter_mut().map(|(_, w)| w)) {
            if state.surface.is_none() {
                continue;
            }
            state.surface = None;
            let Some(window) = state.window.clone() else {
                continue;
            };
            let size = window.inner_size();
            match renderer.add_surface(window.clone(), size.width.max(1), size.height.max(1)) {
                Ok(surface) => state.surface = Some(surface),
                Err(err) => log::error!("a window could not be drawn in again: {err:#}"),
            }
            state.build_dirty = true;
            window.request_redraw();
        }
    }

    /// Fits the surface of the window in front to its size now.
    pub(super) fn resize_surface(&mut self, width: u32, height: u32) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match self.win.surface.as_mut() {
            Some(surface) => renderer.resize_surface(surface, width, height),
            None => renderer.resize(width, height),
        }
    }
}

/// The winit id of the window a state belongs to; a stand-in when it has none yet.
fn window_id_of<A: Application>(state: &WindowState<A>) -> WindowId {
    state
        .window
        .as_ref()
        .map_or(WindowId::dummy(), |window| window.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loop_wakes_for_the_window_that_needs_it_first() {
        let now = Instant::now();
        let later = now + std::time::Duration::from_millis(500);
        assert_eq!(sooner(None, ControlFlow::Wait), ControlFlow::Wait);
        assert_eq!(
            sooner(Some(ControlFlow::Wait), ControlFlow::WaitUntil(later)),
            ControlFlow::WaitUntil(later)
        );
        assert_eq!(
            sooner(
                Some(ControlFlow::WaitUntil(later)),
                ControlFlow::WaitUntil(now)
            ),
            ControlFlow::WaitUntil(now)
        );
        assert_eq!(
            sooner(Some(ControlFlow::WaitUntil(now)), ControlFlow::Wait),
            ControlFlow::WaitUntil(now)
        );
        assert_eq!(
            sooner(Some(ControlFlow::Poll), ControlFlow::Wait),
            ControlFlow::Poll
        );
    }
}
