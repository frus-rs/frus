// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **What another application shares into this one** (milestone 801): text, a subject and
//! files sent from the system's share sheet.
//!
//! On **Android** an application that declares an intent filter for `ACTION_SEND` (see the
//! platform guide) is listed in other applications' share sheets. Picking it opens the
//! application — or hands the share to the running one, through frus's activity — and the
//! share reaches [`Application::on_share`](crate::Application::on_share), or the handler a
//! component application gave [`WidgetApp::on_share`](crate::WidgetApp::on_share). Files come
//! as `content://` addresses only the receiving activity may read; each is **copied into the
//! application's cache folder** first, and [`SharedContent::files`] holds the copies.
//!
//! The other platforms have no such sheet into an application: nothing arrives there.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

/// What another application shared into this one: any of text, a subject and files.
///
/// ```
/// use frus_shell::{Application, Command, SharedContent};
/// # use frus_widgets::{text, Theme, Widget};
///
/// struct Notes {
///     draft: String,
/// }
/// impl Application for Notes {
///     type Message = ();
///     fn update(&mut self, _: ()) -> Command<()> {
///         Command::none()
///     }
///     fn view(&self, _: &Theme) -> Box<dyn Widget<()>> {
///         Box::new(text(self.draft.clone()))
///     }
///     // A note shared from another application becomes the draft.
///     fn on_share(&mut self, shared: SharedContent) -> Command<()> {
///         if let Some(text) = shared.text {
///             self.draft = text;
///         }
///         Command::none()
///     }
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SharedContent {
    /// The text shared (`EXTRA_TEXT` on Android): a message, a link.
    pub text: Option<String>,
    /// Its subject or title (`EXTRA_SUBJECT`), when the sender gave one.
    pub subject: Option<String>,
    /// The type the sender declared: `text/plain`, `image/jpeg`, `image/*` for several images.
    pub mime_type: Option<String>,
    /// The files shared, copied into the application's cache folder: the application may read,
    /// move or delete them; the system may clear the cache when space runs short.
    pub files: Vec<PathBuf>,
}

impl SharedContent {
    /// Whether nothing at all came: no text, no subject, no file.
    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.subject.is_none() && self.files.is_empty()
    }
}

/// The shares received since the last frame, oldest first.
static RECEIVED: Mutex<VecDeque<SharedContent>> = Mutex::new(VecDeque::new());

/// Queues a share for the next frame. An empty one is dropped.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn push(shared: SharedContent) {
    if shared.is_empty() {
        return;
    }
    RECEIVED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push_back(shared);
}

/// The shares received since the last call, oldest first.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn take() -> Vec<SharedContent> {
    let mut queue = RECEIVED.lock().unwrap_or_else(|e| e.into_inner());
    if queue.is_empty() {
        return Vec::new();
    }
    queue.drain(..).collect()
}

/// A file name that is safe to create in a folder: the last segment of `name`, with anything a
/// file system could take for a separator or a device replaced, and never empty or a dot name.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn safe_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "shared".to_string()
    } else {
        cleaned.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_are_taken_once_in_order_and_empty_ones_dropped() {
        // The queue is the process's: take what another test may have left first.
        let _ = take();
        push(SharedContent {
            text: Some("first".into()),
            ..SharedContent::default()
        });
        push(SharedContent::default());
        push(SharedContent {
            files: vec![PathBuf::from("/cache/b.jpg")],
            mime_type: Some("image/jpeg".into()),
            ..SharedContent::default()
        });
        let taken = take();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].text.as_deref(), Some("first"));
        assert_eq!(taken[1].files, [PathBuf::from("/cache/b.jpg")]);
        assert!(take().is_empty());
    }

    #[test]
    fn shared_file_names_cannot_leave_their_folder() {
        assert_eq!(safe_file_name("IMG_0042.jpg"), "IMG_0042.jpg");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("primary:DCIM/a?b.png"), "a_b.png");
        assert_eq!(safe_file_name(".."), "shared");
        assert_eq!(safe_file_name(""), "shared");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        assert_eq!(safe_file_name("a\u{0}b"), "a_b");
    }
}
