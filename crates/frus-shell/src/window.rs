// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **More windows than one** (milestone 827, #218): a window is a value the application's
//! state describes, not a handle it keeps.
//!
//! The application lists the windows it wants open besides its main one —
//! [`Application::windows`](crate::Application::windows) — and builds each one's interface —
//! [`Application::window_view`](crate::Application::window_view). The shell compares the list
//! with the windows it has after every turn of the loop: a window newly listed is opened, one
//! no longer listed is closed, and a title that changed is set. Closing a window by its close
//! button only asks: its [`on_close`](Window::new) message is sent, and the window goes when
//! the state stops listing it.

/// **A window the application wants open**, besides its main one (milestone 827).
///
/// Listed by [`Application::windows`](crate::Application::windows), from the state: opening a
/// window is a message that adds it to the state, closing one a message that takes it out.
///
/// ```
/// use frus_shell::Window;
///
/// #[derive(Clone)]
/// enum Msg {
///     CloseInspector,
/// }
///
/// let inspector: Window<Msg> = Window::new("inspector", Msg::CloseInspector)
///     .title("Inspector")
///     .size(420.0, 560.0);
/// assert_eq!(inspector.id(), "inspector");
/// ```
#[derive(Clone, Debug)]
pub struct Window<Msg> {
    id: String,
    title: Option<String>,
    size: Option<(f32, f32)>,
    min_size: Option<(f32, f32)>,
    on_close: Msg,
}

impl<Msg> Window<Msg> {
    /// A window known as `id` — what [`window_view`](crate::Application::window_view) is
    /// asked to build for, and what keeps it the same window from one list to the next — and
    /// the message its close button sends.
    ///
    /// The close button does not close the window by itself: the message's handler takes the
    /// window out of the state, or keeps it (a document with unsaved changes asks first).
    pub fn new(id: impl Into<String>, on_close: impl Into<Msg>) -> Self {
        Self {
            id: id.into(),
            title: None,
            size: None,
            min_size: None,
            on_close: on_close.into(),
        }
    }

    /// The window's title: the application's own when none is given. A title that changes in
    /// the state changes on the window.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The size the window opens at, in logical pixels: the system's choice when none is given.
    /// Read when the window opens; the reader resizes it after that.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Some((width, height));
        self
    }

    /// The smallest the reader may make the window, in logical pixels: the main window's
    /// minimum when none is given.
    pub fn min_size(mut self, width: f32, height: f32) -> Self {
        self.min_size = Some((width, height));
        self
    }

    /// What the window is known as.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The title it asks for, if any.
    pub fn title_text(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The size it asks to open at, if any.
    pub fn opening_size(&self) -> Option<(f32, f32)> {
        self.size
    }

    /// The smallest size it asks for, if any.
    pub fn smallest_size(&self) -> Option<(f32, f32)> {
        self.min_size
    }

    /// The message its close button sends.
    pub fn close_message(&self) -> &Msg {
        &self.on_close
    }

    /// The same window, sending another kind of message: what a component application's
    /// builder needs to hand its windows to the shell.
    pub fn map<N>(self, f: impl FnOnce(Msg) -> N) -> Window<N> {
        Window {
            id: self.id,
            title: self.title,
            size: self.size,
            min_size: self.min_size,
            on_close: f(self.on_close),
        }
    }
}

/// What the shell does to bring its windows to what the application lists (milestone 827):
/// the windows to open, those to close, and the titles to set. Pure, so that it is tested
/// without a display.
#[derive(Debug, Default, PartialEq)]
#[cfg_attr(not(desktop), allow(dead_code))]
pub(crate) struct WindowChanges {
    /// The positions, in the list, of the windows to open.
    pub open: Vec<usize>,
    /// The ids of the windows to close.
    pub close: Vec<String>,
    /// The ids of the windows whose title is to change, with the new one.
    pub retitle: Vec<(String, String)>,
}

/// Compares what is open — each window's id and title — with what the application `wanted`;
/// a window with no title of its own is titled `fallback`, the application's. A window listed
/// twice is opened once: the first wins.
#[cfg_attr(not(desktop), allow(dead_code))]
pub(crate) fn window_changes<Msg>(
    open: &[(&str, &str)],
    wanted: &[Window<Msg>],
    fallback: &str,
) -> WindowChanges {
    let mut changes = WindowChanges::default();
    let mut seen = std::collections::HashSet::new();
    for (at, window) in wanted.iter().enumerate() {
        if !seen.insert(window.id()) {
            continue;
        }
        let title = window.title_text().unwrap_or(fallback);
        match open.iter().find(|(id, _)| *id == window.id()) {
            None => changes.open.push(at),
            Some((_, shown)) if *shown != title => changes
                .retitle
                .push((window.id().to_string(), title.to_string())),
            Some(_) => {}
        }
    }
    for (id, _) in open {
        if !seen.contains(id) {
            changes.close.push(id.to_string());
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(ids: &[(&str, Option<&str>)]) -> Vec<Window<()>> {
        ids.iter()
            .map(|(id, title)| {
                let window = Window::new(*id, ());
                match title {
                    Some(title) => window.title(*title),
                    None => window,
                }
            })
            .collect()
    }

    #[test]
    fn a_window_newly_listed_opens_and_one_no_longer_listed_closes() {
        let changes = window_changes(
            &[("a", "App"), ("b", "App")],
            &listed(&[("b", None), ("c", None)]),
            "App",
        );
        assert_eq!(changes.open, vec![1]);
        assert_eq!(changes.close, vec!["a".to_string()]);
        assert!(changes.retitle.is_empty());
    }

    #[test]
    fn a_title_that_changed_in_the_state_changes_on_the_window() {
        let changes = window_changes(
            &[("doc", "Untitled")],
            &listed(&[("doc", Some("Report.txt"))]),
            "App",
        );
        assert_eq!(
            changes.retitle,
            vec![("doc".to_string(), "Report.txt".to_string())]
        );
        assert!(changes.open.is_empty() && changes.close.is_empty());
    }

    #[test]
    fn a_window_without_a_title_takes_the_applications() {
        let changes = window_changes(&[("doc", "App")], &listed(&[("doc", None)]), "App");
        assert_eq!(changes, WindowChanges::default());
    }

    #[test]
    fn a_window_listed_twice_opens_once() {
        let changes = window_changes::<()>(&[], &listed(&[("a", None), ("a", None)]), "App");
        assert_eq!(changes.open, vec![0]);
    }

    #[test]
    fn nothing_listed_and_nothing_open_changes_nothing() {
        assert_eq!(
            window_changes::<()>(&[], &[], "App"),
            WindowChanges::default()
        );
    }
}
