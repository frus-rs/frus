// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Shortcuts and actions**: a key combination, what it means, and who answers.
//!
//! The reference splits this in two on purpose, and the split is the whole design:
//!
//! - [`Shortcuts`] maps a **keystroke to an intent** — *Ctrl+S means «save»*. It knows
//!   nothing about what saving does.
//! - [`Actions`] maps an **intent to a message** — *«save» means `Msg::Save`*. It knows
//!   nothing about which key got here.
//!
//! The point of the indirection is that the two can be replaced independently. A dialog
//! can rebind the keys without touching the handlers; a subtree can answer «save»
//! differently from the page around it, and the innermost answer wins — the same rule as
//! focus, resolved the same way.
//!
//! When there is no reason to name an intent, [`CallbackShortcuts`] binds the keystroke
//! straight to a message and skips the ceremony.
//!
//! ```ignore
//! Actions::new(child)
//!     .action(Intent("save"), Msg::Save)
//!     .action(Intent("close"), Msg::Close)
//! // …with, anywhere inside it:
//! Shortcuts::new(form)
//!     .bind(KeyStroke::new(ShortcutKey::Char('s')).ctrl(), Intent("save"))
//! ```
//!
//! ## Which scope answers
//!
//! A binding applies when **focus is inside the subtree that declared it**, and the
//! innermost such subtree wins. With nothing focused, every scope is a candidate and the
//! innermost declared still wins.
//!
//! ## Typing beats a bare letter
//!
//! A keystroke with no Ctrl, Alt or Meta goes to the focused text field first, if there is
//! one. Otherwise a `Shortcuts` binding on the letter `a` would make every field in the
//! application impossible to type in — the reference reaches the same conclusion through
//! its text-editing shortcuts taking priority in the focused scope.
//!
//! ## Not here: `ShortcutRegistrar`
//!
//! The reference has a registry a deep widget can *register into at runtime*, because its
//! tree is retained and a widget's build is not re-run on every frame. Here the view is
//! rebuilt from the state each frame, so declaring the binding where it applies is the
//! same thing with none of the bookkeeping. That is an architectural difference, not a
//! missing feature.

use frus_core::{Rect, Scene};
use frus_layout::Style;

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::{sizing_of, Widget};

/// A key a shortcut can be bound to — a **general** vocabulary, unlike
/// [`Key`](crate::Key), which is the one text editing needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutKey {
    /// A printable key, matched **case-insensitively**: `Ctrl+S` and `Ctrl+s` are the
    /// same shortcut, and the caller should not have to think about Caps Lock.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Space.
    Space,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// A function key, `F(1)` through `F(24)`.
    F(u8),
}

/// A key plus its modifiers — both the **pattern** a shortcut is bound to and the
/// **description** of what was actually pressed. One type, because they are compared to
/// each other and a second one would only be the first with the fields renamed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStroke {
    /// The key itself.
    pub key: ShortcutKey,
    /// Control, on every keyboard — a Mac's too, where it is not the key of the shortcuts.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt / Option.
    pub alt: bool,
    /// The platform's "logo" key: Windows' key, a Mac's Command. A shortcut meant for the
    /// key a Mac's hands reach for — Cmd+S where everyone else has Ctrl+S — is bound with
    /// [`primary`](Self::primary) instead.
    pub meta: bool,
    /// The **platform's own modifier**: Cmd on an Apple keyboard, Ctrl on every other
    /// (milestone 852). Only a bound stroke says it; a pressed one names the keys that were
    /// held, and [`resolve`](Self::resolve) turns one into the other.
    pub primary: bool,
}

impl KeyStroke {
    /// A bare key, no modifiers.
    pub const fn new(key: ShortcutKey) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            primary: false,
        }
    }

    /// With the **platform's own modifier** held: Cmd on an Apple keyboard, Ctrl on every
    /// other — the key of Copy, Save and Find. One binding answers both:
    ///
    /// ```
    /// # use frus_widgets::{KeyStroke, ShortcutKey, TargetPlatform};
    /// let save = KeyStroke::new(ShortcutKey::Char('s')).primary();
    /// let cmd_s = KeyStroke::new(ShortcutKey::Char('s')).meta();
    /// let ctrl_s = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
    /// assert!(cmd_s.matches_on(&save, TargetPlatform::MacOs));
    /// assert!(ctrl_s.matches_on(&save, TargetPlatform::Windows));
    /// assert!(!ctrl_s.matches_on(&save, TargetPlatform::MacOs));
    /// ```
    ///
    /// Which keyboard that is comes from [`keyboard_platform`](crate::keyboard_platform):
    /// the browser's operating system on the web, so a page read on a Mac answers Cmd.
    pub const fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    /// The stroke with its primary modifier spelled out for `keyboard`'s platform: `meta`
    /// on an Apple keyboard, `ctrl` on any other. A stroke without one is returned as it is.
    pub const fn resolve(mut self, keyboard: crate::TargetPlatform) -> Self {
        if self.primary {
            self.primary = false;
            if keyboard.is_apple() {
                self.meta = true;
            } else {
                self.ctrl = true;
            }
        }
        self
    }

    /// How the stroke is **written for people** on a keyboard of `keyboard`'s platform, as a
    /// menu or a command palette shows it: `⌘⇧Z` on an Apple keyboard, `Ctrl+Shift+Z` on any
    /// other — the primary modifier spelled as that keyboard has it.
    ///
    /// ```
    /// # use frus_widgets::{KeyStroke, ShortcutKey, TargetPlatform};
    /// let redo = KeyStroke::new(ShortcutKey::Char('z')).primary().shift();
    /// assert_eq!(redo.label_on(TargetPlatform::MacOs), "⇧⌘Z");
    /// assert_eq!(redo.label_on(TargetPlatform::Linux), "Ctrl+Shift+Z");
    /// ```
    pub fn label_on(&self, keyboard: crate::TargetPlatform) -> String {
        let stroke = self.resolve(keyboard);
        let apple = keyboard.is_apple();
        let key = match stroke.key {
            ShortcutKey::Char(c) => c.to_uppercase().collect(),
            ShortcutKey::Enter => (if apple { "↩" } else { "Enter" }).to_string(),
            // The bundled sans has no glyph for U+238B; the bundled monospace face draws it.
            ShortcutKey::Escape => (if apple { "⎋" } else { "Esc" }).to_string(),
            ShortcutKey::Tab => (if apple { "⇥" } else { "Tab" }).to_string(),
            ShortcutKey::Space => "Space".to_string(),
            ShortcutKey::Backspace => (if apple { "⌫" } else { "Backspace" }).to_string(),
            ShortcutKey::Delete => (if apple { "⌦" } else { "Delete" }).to_string(),
            ShortcutKey::Up => "↑".to_string(),
            ShortcutKey::Down => "↓".to_string(),
            ShortcutKey::Left => "←".to_string(),
            ShortcutKey::Right => "→".to_string(),
            ShortcutKey::Home => "Home".to_string(),
            ShortcutKey::End => "End".to_string(),
            ShortcutKey::PageUp => "PgUp".to_string(),
            ShortcutKey::PageDown => "PgDn".to_string(),
            ShortcutKey::F(n) => format!("F{n}"),
        };
        if apple {
            // Apple's order: Control, Option, Shift, Command, and no separator.
            let mut out = String::new();
            for (held, glyph) in [
                (stroke.ctrl, '⌃'),
                (stroke.alt, '⌥'),
                (stroke.shift, '⇧'),
                (stroke.meta, '⌘'),
            ] {
                if held {
                    out.push(glyph);
                }
            }
            out + &key
        } else {
            let mut parts: Vec<&str> = Vec::new();
            for (held, name) in [
                (stroke.ctrl, "Ctrl"),
                (stroke.alt, "Alt"),
                (stroke.shift, "Shift"),
                (stroke.meta, "Meta"),
            ] {
                if held {
                    parts.push(name);
                }
            }
            parts.push(&key);
            parts.join("+")
        }
    }

    /// With Control held.
    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// With Shift held.
    pub const fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// With Alt held.
    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// With the logo key held.
    pub const fn meta(mut self) -> Self {
        self.meta = true;
        self
    }

    /// Does this stroke carry a modifier that makes it a **command** rather than typing?
    /// Shift alone does not: `Shift+A` is a capital A.
    pub const fn is_command(&self) -> bool {
        self.ctrl || self.alt || self.meta || self.primary
    }

    /// Do a pressed stroke and a bound one match? Letters are compared without case, so a
    /// binding written `Char('s')` answers a Shift-less `S` from a keyboard with Caps Lock
    /// on. A [`primary`](Self::primary) modifier on either is read for the machine's
    /// keyboard, [`keyboard_platform`](crate::keyboard_platform); see
    /// [`matches_on`](Self::matches_on) to name another.
    pub fn matches(&self, bound: &KeyStroke) -> bool {
        self.matches_on(bound, crate::keyboard_platform())
    }

    /// [`matches`](Self::matches), with the primary modifier read for `keyboard`'s
    /// platform: Cmd on macOS and iOS, Ctrl elsewhere.
    pub fn matches_on(&self, bound: &KeyStroke, keyboard: crate::TargetPlatform) -> bool {
        let (pressed, bound) = (self.resolve(keyboard), bound.resolve(keyboard));
        pressed.same(&bound)
    }

    /// The comparison itself, of two strokes with no primary modifier left in them.
    fn same(&self, bound: &KeyStroke) -> bool {
        let same_key = match (self.key, bound.key) {
            (ShortcutKey::Char(a), ShortcutKey::Char(b)) => {
                a.eq_ignore_ascii_case(&b) || a.to_lowercase().eq(b.to_lowercase())
            }
            (a, b) => a == b,
        };
        same_key
            && self.ctrl == bound.ctrl
            && self.shift == bound.shift
            && self.alt == bound.alt
            && self.meta == bound.meta
    }
}

/// What a keystroke **means**, named rather than implemented: `Intent("save")`.
///
/// A string rather than a type, because the whole point is that the widget binding the key
/// and the widget answering it need not know about one another — and a name reads in a
/// dump, which a type id does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intent(pub &'static str);

impl Intent {
    /// Copies the selection — of the focused text field, or of a selection area.
    pub const COPY: Intent = Intent("frus.copy");
    /// Cuts the focused text field's selection.
    pub const CUT: Intent = Intent("frus.cut");
    /// Pastes into the focused text field.
    pub const PASTE: Intent = Intent("frus.paste");
    /// Selects the whole of the focused text field, or of a selection area.
    pub const SELECT_ALL: Intent = Intent("frus.select-all");
    /// Undoes the focused text field's last edit.
    pub const UNDO: Intent = Intent("frus.undo");
    /// Redoes the focused text field's last undone edit.
    pub const REDO: Intent = Intent("frus.redo");
    /// **Takes the key and does nothing**: bound in a [`Shortcuts`], it switches off whatever
    /// the framework would have done with that key — a default editing shortcut, say —
    /// inside the subtree.
    pub const DO_NOTHING: Intent = Intent("frus.do-nothing");

    /// The framework's own editing intents, which a text field and a selection area answer
    /// without an [`Actions`]: Copy, Cut, Paste, Select all, Undo and Redo.
    pub const EDITING: [Intent; 6] = [
        Intent::COPY,
        Intent::CUT,
        Intent::PASTE,
        Intent::SELECT_ALL,
        Intent::UNDO,
        Intent::REDO,
    ];

    /// Whether this is one of the framework's [editing intents](Self::EDITING).
    pub fn is_editing(self) -> bool {
        Intent::EDITING.contains(&self)
    }
}

/// The framework's **default editing shortcuts** for a keyboard of `keyboard`'s platform —
/// what a text field and a selection area answer when no [`Shortcuts`] around them binds
/// the key first (milestone 852).
///
/// Written with the [primary modifier](KeyStroke::primary), so one table serves every
/// keyboard: Cmd+C on a Mac, Ctrl+C elsewhere. Redo is primary+Shift+Z everywhere, and also
/// Ctrl+Y off Apple keyboards — a browser on a Mac opens its history on Cmd+Y.
///
/// **Overriding it** is binding the key in a [`Shortcuts`] above the field: to another
/// meaning of the application's, to [`Intent::DO_NOTHING`] to switch it off, or another key
/// to one of these intents — `Ctrl+Insert` to [`Intent::COPY`], say.
pub fn default_editing_shortcuts(keyboard: crate::TargetPlatform) -> Vec<(KeyStroke, Intent)> {
    let key = |c| KeyStroke::new(ShortcutKey::Char(c)).primary();
    let mut table = vec![
        (key('c'), Intent::COPY),
        (key('x'), Intent::CUT),
        (key('v'), Intent::PASTE),
        (key('a'), Intent::SELECT_ALL),
        (key('z'), Intent::UNDO),
        (key('z').shift(), Intent::REDO),
    ];
    if !keyboard.is_apple() {
        table.push((key('y'), Intent::REDO));
    }
    table
}

/// The shared body: one child, its box, and one of the three tables.
struct Wrapper<Msg> {
    child: Vec<Box<dyn Widget<Msg>>>,
    focusable: bool,
    shortcuts: Vec<(KeyStroke, Intent)>,
    callbacks: Vec<(KeyStroke, Msg)>,
    actions: Vec<(Intent, Msg)>,
    listeners: Vec<(Intent, Msg)>,
}

impl<Msg> Wrapper<Msg> {
    fn new(child: Box<dyn Widget<Msg>>) -> Self {
        Self {
            child: vec![child],
            focusable: false,
            shortcuts: Vec::new(),
            callbacks: Vec::new(),
            actions: Vec::new(),
            listeners: Vec::new(),
        }
    }
}

impl<Msg: Clone> Widget<Msg> for Wrapper<Msg> {
    fn style(&self) -> Style {
        sizing_of(self.child[0].style())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        sizing_of(self.child[0].style_themed(theme))
    }

    fn build_themed(&self, theme: &Theme) {
        self.child[0].build_themed(theme);
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.child
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn focusable(&self) -> bool {
        self.focusable
    }

    fn shortcut_bindings(&self) -> &[(KeyStroke, Intent)] {
        &self.shortcuts
    }

    fn shortcut_callbacks(&self) -> &[(KeyStroke, Msg)] {
        &self.callbacks
    }

    fn action_bindings(&self) -> &[(Intent, Msg)] {
        &self.actions
    }

    fn action_listeners(&self) -> &[(Intent, Msg)] {
        &self.listeners
    }
}

/// Binds **keystrokes to intents** for its subtree. See the module's documentation for
/// why that is two steps rather than one.
pub struct Shortcuts<Msg = crate::callback::Callback>(Wrapper<Msg>);

impl<Msg> Shortcuts<Msg> {
    /// A shortcut scope around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self(Wrapper::new(Box::new(child)))
    }

    /// Binds a keystroke to an intent. A later binding for the same stroke replaces an
    /// earlier one, so a caller can override a table it did not write.
    pub fn bind(mut self, stroke: KeyStroke, intent: Intent) -> Self {
        self.0.shortcuts.retain(|(s, _)| !s.matches(&stroke));
        self.0.shortcuts.push((stroke, intent));
        self
    }
}

/// Binds **keystrokes straight to messages**, for when naming an intent would be
/// ceremony — the reference's `CallbackShortcuts`.
pub struct CallbackShortcuts<Msg = crate::callback::Callback>(Wrapper<Msg>);

impl<Msg: Clone> CallbackShortcuts<Msg> {
    /// A shortcut scope around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self(Wrapper::new(Box::new(child)))
    }

    /// Binds a keystroke to the message it sends.
    pub fn bind(mut self, stroke: KeyStroke, msg: Msg) -> Self {
        self.0.callbacks.retain(|(s, _)| !s.matches(&stroke));
        self.0.callbacks.push((stroke, msg));
        self
    }
}

/// Answers **intents** for its subtree: the innermost `Actions` that has the intent is
/// the one that answers it.
pub struct Actions<Msg = crate::callback::Callback>(Wrapper<Msg>);

impl<Msg> Actions<Msg> {
    /// An action scope around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self(Wrapper::new(Box::new(child)))
    }

    /// What this scope does about `intent`.
    pub fn action(mut self, intent: Intent, msg: Msg) -> Self {
        self.0.actions.push((intent, msg));
        self
    }
}

/// **Watches** intents without answering them: its message is sent as well as, not
/// instead of, whatever the answering [`Actions`] sends.
///
/// For the things that want to know an action happened — an undo stack, a telemetry
/// counter, a status line — without becoming the one that performs it.
pub struct ActionListener<Msg = crate::callback::Callback>(Wrapper<Msg>);

impl<Msg> ActionListener<Msg> {
    /// A listener around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self(Wrapper::new(Box::new(child)))
    }

    /// Also send `msg` whenever `intent` is invoked inside this subtree.
    pub fn on(mut self, intent: Intent, msg: Msg) -> Self {
        self.0.listeners.push((intent, msg));
        self
    }
}

/// A **focus stop that carries its own keys**: focusable, with its own shortcut and action
/// tables, so a control can answer the keyboard for itself while it has focus.
///
/// The reference's `FocusableActionDetector`, and the sixth of the focus widgets — it
/// waited for this milestone because it is made of shortcuts as much as of focus. It is
/// the composition, given one name because the three are always wanted together: a menu
/// item that answers Enter, a card that answers Delete, a canvas that answers arrows.
pub struct FocusableActionDetector<Msg = crate::callback::Callback>(Wrapper<Msg>);

impl<Msg: Clone> FocusableActionDetector<Msg> {
    /// A focusable, key-answering wrapper around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        let mut w = Wrapper::new(Box::new(child));
        w.focusable = true;
        Self(w)
    }

    /// Whether it takes focus at all. A disabled control should say `false` here rather
    /// than keep an inert focus stop that Tab still lands on.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.0.focusable = enabled;
        self
    }

    /// Binds a keystroke to an intent, for while focus is inside.
    pub fn bind(mut self, stroke: KeyStroke, intent: Intent) -> Self {
        self.0.shortcuts.retain(|(s, _)| !s.matches(&stroke));
        self.0.shortcuts.push((stroke, intent));
        self
    }

    /// Binds a keystroke straight to a message.
    pub fn on_key(mut self, stroke: KeyStroke, msg: Msg) -> Self {
        self.0.callbacks.retain(|(s, _)| !s.matches(&stroke));
        self.0.callbacks.push((stroke, msg));
        self
    }

    /// What this control does about `intent`.
    pub fn action(mut self, intent: Intent, msg: Msg) -> Self {
        self.0.actions.push((intent, msg));
        self
    }
}

/// Hands **every keystroke** in its subtree to a closure — the reference's
/// `KeyboardListener`, and the escape hatch for anything the tables above cannot say.
pub struct KeyboardListener<Msg = crate::callback::Callback> {
    child: Vec<Box<dyn Widget<Msg>>>,
    /// Shared rather than borrowed: the interface the walk produces outlives the tree it
    /// was built from, so a closure it carries has to be owned.
    #[allow(clippy::type_complexity)]
    on_key: std::rc::Rc<dyn Fn(KeyStroke) -> Option<Msg>>,
}

impl<Msg> KeyboardListener<Msg> {
    /// Wraps `child`, calling `on_key` for each keystroke while focus is inside it.
    /// Returning `None` lets the keystroke carry on to the scopes around it.
    pub fn new(
        child: impl Widget<Msg> + 'static,
        on_key: impl Fn(KeyStroke) -> Option<Msg> + 'static,
    ) -> Self {
        Self {
            child: vec![Box::new(child)],
            on_key: std::rc::Rc::new(on_key),
        }
    }
}

impl<Msg: Clone> Widget<Msg> for KeyboardListener<Msg> {
    fn style(&self) -> Style {
        sizing_of(self.child[0].style())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        sizing_of(self.child[0].style_themed(theme))
    }

    fn build_themed(&self, theme: &Theme) {
        self.child[0].build_themed(theme);
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.child
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn on_keystroke(&self) -> Option<std::rc::Rc<dyn Fn(KeyStroke) -> Option<Msg>>> {
        Some(std::rc::Rc::clone(&self.on_key))
    }
}

/// The four table-carrying wrappers are the same box with a different table filled in.
macro_rules! delegate {
    ($($ty:ident),* $(,)?) => {$(
        impl<Msg: Clone> Widget<Msg> for $ty<Msg> {
            fn style(&self) -> Style {
                Widget::<Msg>::style(&self.0)
            }
            fn style_themed(&self, theme: &Theme) -> Style {
                Widget::<Msg>::style_themed(&self.0, theme)
            }
            fn build_themed(&self, theme: &Theme) {
                Widget::<Msg>::build_themed(&self.0, theme)
            }
            fn children(&self) -> &[Box<dyn Widget<Msg>>] {
                Widget::<Msg>::children(&self.0)
            }
            fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
                Widget::<Msg>::paint(&self.0, bounds, status, theme, scene)
            }
            fn on_click(&self) -> Option<Msg> {
                Widget::<Msg>::on_click(&self.0)
            }
            fn focusable(&self) -> bool {
                Widget::<Msg>::focusable(&self.0)
            }
            fn shortcut_bindings(&self) -> &[(KeyStroke, Intent)] {
                Widget::<Msg>::shortcut_bindings(&self.0)
            }
            fn shortcut_callbacks(&self) -> &[(KeyStroke, Msg)] {
                Widget::<Msg>::shortcut_callbacks(&self.0)
            }
            fn action_bindings(&self) -> &[(Intent, Msg)] {
                Widget::<Msg>::action_bindings(&self.0)
            }
            fn action_listeners(&self) -> &[(Intent, Msg)] {
                Widget::<Msg>::action_listeners(&self.0)
            }
        }
    )*};
}

delegate!(
    Shortcuts,
    CallbackShortcuts,
    Actions,
    ActionListener,
    FocusableActionDetector,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        SavePage,
        SaveDialog,
        Edited(String),
    }

    #[test]
    fn a_letter_matches_without_case_and_a_modifier_does_not() {
        let bound = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
        assert!(KeyStroke::new(ShortcutKey::Char('S'))
            .ctrl()
            .matches(&bound));
        assert!(KeyStroke::new(ShortcutKey::Char('s'))
            .ctrl()
            .matches(&bound));
        assert!(
            !KeyStroke::new(ShortcutKey::Char('s')).matches(&bound),
            "no Ctrl"
        );
        assert!(
            !KeyStroke::new(ShortcutKey::Char('s'))
                .ctrl()
                .shift()
                .matches(&bound),
            "Shift is part of the pattern"
        );
    }

    /// Shift alone is typing, not a command — otherwise a capital letter could not be
    /// typed into a field that sits under a shortcut scope.
    #[test]
    fn shift_alone_is_not_a_command() {
        assert!(!KeyStroke::new(ShortcutKey::Char('a')).shift().is_command());
        assert!(KeyStroke::new(ShortcutKey::Char('a')).ctrl().is_command());
        assert!(KeyStroke::new(ShortcutKey::Char('a')).alt().is_command());
        assert!(KeyStroke::new(ShortcutKey::Char('a')).meta().is_command());
    }

    /// The two-step resolution end to end: a key names an intent, and the innermost
    /// `Actions` that answers it supplies the message.
    #[test]
    fn a_key_names_an_intent_and_the_nearest_answer_wins() {
        use crate::runtime::Runtime;
        use crate::ui::build_ui;
        use frus_core::Size;

        let save = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
        let field = |name: &str| {
            crate::TextField::new(name.to_string())
                .label(name.to_string())
                .on_input(Msg::Edited)
        };

        // A page that saves, holding a dialog that saves differently.
        let tree = Actions::new(
            Shortcuts::new(crate::column![
                field("page"),
                Actions::new(field("dialog")).action(Intent("save"), Msg::SaveDialog),
            ])
            .bind(save, Intent("save")),
        )
        .action(Intent("save"), Msg::SavePage);

        let ui = build_ui(
            &tree,
            Size::new(400.0, 400.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let stops: Vec<_> = ui.focusable_ids().collect();
        assert_eq!(stops.len(), 2);

        assert_eq!(ui.keystroke(save, Some(stops[0])), vec![Msg::SavePage]);
        assert_eq!(
            ui.keystroke(save, Some(stops[1])),
            vec![Msg::SaveDialog],
            "the innermost answer wins"
        );
        assert!(
            ui.keystroke(KeyStroke::new(ShortcutKey::Char('s')), Some(stops[0]))
                .is_empty(),
            "without Ctrl it is typing, not a command"
        );
    }

    /// An intent nobody answers is inert, not an error: a key bound to a meaning the
    /// current screen has no answer for should do nothing.
    #[test]
    fn an_unanswered_intent_does_nothing() {
        use crate::runtime::Runtime;
        use crate::ui::build_ui;
        use frus_core::Size;

        let save = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
        let tree: Shortcuts<Msg> =
            Shortcuts::new(crate::Container::new().width(10.0)).bind(save, Intent("save"));
        let ui = build_ui(
            &tree,
            Size::new(100.0, 100.0),
            &Runtime::default(),
            &Theme::default(),
        );
        assert!(ui.keystroke(save, None).is_empty());
    }

    /// The sixth focus widget: a stop that answers its own keys while it has them.
    #[test]
    fn a_detector_is_a_focus_stop_that_answers_its_own_keys() {
        use crate::runtime::Runtime;
        use crate::ui::build_ui;
        use frus_core::Size;

        let del = KeyStroke::new(ShortcutKey::Delete);
        let card = FocusableActionDetector::new(crate::Container::new().width(40.0).height(40.0))
            .on_key(del, Msg::SaveDialog);
        assert!(Widget::<Msg>::focusable(&card));

        let ui = build_ui(
            &card,
            Size::new(100.0, 100.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let stop = ui.focusable_ids().next().expect("its own stop");
        assert_eq!(ui.keystroke(del, Some(stop)), vec![Msg::SaveDialog]);

        // Disabled, it is not a stop at all — rather than an inert one Tab still lands on.
        let off: FocusableActionDetector<Msg> =
            FocusableActionDetector::new(crate::Container::new()).enabled(false);
        assert!(!Widget::<Msg>::focusable(&off));
    }

    /// **The primary modifier is Cmd on Apple's platforms and Ctrl on every other**
    /// (milestone 852, L-037), and a stroke without one is left as it was.
    #[test]
    fn the_primary_modifier_is_cmd_on_apple_platforms_and_ctrl_elsewhere() {
        use crate::TargetPlatform;
        let save = KeyStroke::new(ShortcutKey::Char('s')).primary();
        for platform in TargetPlatform::ALL {
            let resolved = save.resolve(platform);
            assert!(!resolved.primary, "{platform}");
            let apple = matches!(platform, TargetPlatform::MacOs | TargetPlatform::Ios);
            assert_eq!(resolved.meta, apple, "{platform}: Cmd");
            assert_eq!(resolved.ctrl, !apple, "{platform}: Ctrl");

            let cmd_s = KeyStroke::new(ShortcutKey::Char('S')).meta();
            let ctrl_s = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
            assert_eq!(cmd_s.matches_on(&save, platform), apple, "{platform}");
            assert_eq!(ctrl_s.matches_on(&save, platform), !apple, "{platform}");
            assert!(
                !KeyStroke::new(ShortcutKey::Char('s')).matches_on(&save, platform),
                "{platform}: a bare S is typing"
            );
            assert!(
                !ctrl_s.meta().matches_on(&save, platform),
                "{platform}: an extra modifier is another stroke"
            );
        }
        let plain = KeyStroke::new(ShortcutKey::F(5)).alt();
        assert_eq!(plain.resolve(TargetPlatform::MacOs), plain);
        assert!(save.is_command(), "a primary stroke is a command");
    }

    /// **The default editing table**, per platform: the same six intents on primary, and
    /// Ctrl+Y as a second Redo only off Apple keyboards.
    #[test]
    fn the_default_editing_shortcuts_follow_the_keyboard() {
        use crate::TargetPlatform;
        let find = |platform, stroke: KeyStroke| {
            default_editing_shortcuts(platform)
                .into_iter()
                .find(|(bound, _)| stroke.matches_on(bound, platform))
                .map(|(_, intent)| intent)
        };
        let key = |c| KeyStroke::new(ShortcutKey::Char(c));
        for platform in TargetPlatform::ALL {
            let apple = platform.is_apple();
            let held = |stroke: KeyStroke| if apple { stroke.meta() } else { stroke.ctrl() };
            let other = |stroke: KeyStroke| if apple { stroke.ctrl() } else { stroke.meta() };
            for (c, intent) in [
                ('c', Intent::COPY),
                ('X', Intent::CUT),
                ('v', Intent::PASTE),
                ('a', Intent::SELECT_ALL),
                ('z', Intent::UNDO),
            ] {
                assert_eq!(find(platform, held(key(c))), Some(intent), "{platform} {c}");
                assert_eq!(find(platform, other(key(c))), None, "{platform} {c}");
                assert_eq!(find(platform, key(c)), None, "{platform}: typing {c}");
            }
            assert_eq!(
                find(platform, held(key('z')).shift()),
                Some(Intent::REDO),
                "{platform}"
            );
            assert_eq!(
                find(platform, held(key('y'))),
                (!apple).then_some(Intent::REDO),
                "{platform}: Ctrl+Y, but not Cmd+Y"
            );
        }
        assert!(Intent::EDITING.iter().all(|i| i.is_editing()));
        assert!(!Intent("save").is_editing());
        assert!(!Intent::DO_NOTHING.is_editing());
    }

    /// **One binding written with the primary modifier answers Cmd on a Mac and Ctrl
    /// elsewhere**, through the resolution a frame's interface does — and the resolution
    /// names the intent it found, answered or not.
    #[test]
    fn a_primary_binding_resolves_for_the_keyboard_it_is_typed_on() {
        use crate::runtime::Runtime;
        use crate::ui::build_ui;
        use crate::TargetPlatform;
        use frus_core::Size;

        let tree = Actions::new(
            Shortcuts::new(crate::Container::new().width(10.0))
                .bind(
                    KeyStroke::new(ShortcutKey::Char('s')).primary(),
                    Intent("save"),
                )
                .bind(
                    KeyStroke::new(ShortcutKey::Char('c')).primary(),
                    Intent::COPY,
                ),
        )
        .action(Intent("save"), Msg::SavePage);
        let ui = build_ui(
            &tree,
            Size::new(100.0, 100.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let cmd_s = KeyStroke::new(ShortcutKey::Char('s')).meta();
        let ctrl_s = KeyStroke::new(ShortcutKey::Char('s')).ctrl();
        let mac = TargetPlatform::MacOs;
        let windows = TargetPlatform::Windows;
        assert_eq!(
            ui.resolve_keystroke(cmd_s, None, mac),
            (vec![Msg::SavePage], Some(Intent("save")))
        );
        assert_eq!(ui.resolve_keystroke(ctrl_s, None, mac), (vec![], None));
        assert_eq!(
            ui.resolve_keystroke(ctrl_s, None, windows),
            (vec![Msg::SavePage], Some(Intent("save")))
        );
        assert_eq!(ui.resolve_keystroke(cmd_s, None, windows), (vec![], None));
        // Nobody here answers Copy: the shell does, for the field or the area.
        assert_eq!(
            ui.resolve_keystroke(KeyStroke::new(ShortcutKey::Char('c')).ctrl(), None, windows),
            (vec![], Some(Intent::COPY))
        );
    }

    /// **A stroke is written for people as its keyboard has it.**
    #[test]
    fn a_stroke_is_labelled_as_its_keyboard_writes_it() {
        use crate::TargetPlatform;
        let palette = KeyStroke::new(ShortcutKey::Char('k')).primary();
        assert_eq!(palette.label_on(TargetPlatform::MacOs), "⌘K");
        assert_eq!(palette.label_on(TargetPlatform::Windows), "Ctrl+K");
        let all = KeyStroke::new(ShortcutKey::Enter)
            .ctrl()
            .alt()
            .shift()
            .meta();
        assert_eq!(all.label_on(TargetPlatform::Ios), "⌃⌥⇧⌘↩");
        assert_eq!(
            all.label_on(TargetPlatform::Linux),
            "Ctrl+Alt+Shift+Meta+Enter"
        );
    }

    /// A later binding for the same stroke replaces the earlier one, so a caller can
    /// override a table it did not write rather than fighting it.
    #[test]
    fn rebinding_a_stroke_replaces_it() {
        let s: Shortcuts<()> = Shortcuts::new(crate::Container::new())
            .bind(
                KeyStroke::new(ShortcutKey::Char('s')).ctrl(),
                Intent("save"),
            )
            .bind(
                KeyStroke::new(ShortcutKey::Char('s')).ctrl(),
                Intent("save-as"),
            );
        let table = Widget::<()>::shortcut_bindings(&s);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].1, Intent("save-as"));
    }
}
