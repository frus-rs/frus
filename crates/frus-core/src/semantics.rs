// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Accessibility semantics**: the per-widget annotation (role, label, value,
//! state) that the framework exposes to assistive technology.
//!
//! A frus-native, dependency-free type that **maps** onto `accesskit` at the
//! platform edge (the shell builds an AccessKit tree out of these nodes). This
//! follows the §14 advice: *bake the label into the widgets now, wire AccessKit up
//! afterwards.*

/// An element's semantic **role** (a subset aligned on AccessKit/ARIA roles: what a
/// screen reader announces).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Role {
    /// An element with no role of its own (a layout container).
    #[default]
    None,
    /// Static text (label, paragraph).
    Label,
    /// A heading or title.
    Heading,
    /// An actionable button.
    Button,
    /// A link.
    Link,
    /// A checkbox (`checked` state).
    CheckBox,
    /// A switch (`checked` state).
    Switch,
    /// A radio button (`checked` state).
    RadioButton,
    /// A continuous-value slider (`value` / `min` / `max`).
    Slider,
    /// A text input field (`value` = its contents).
    TextInput,
    /// An image or icon described by its `label`.
    Image,
    /// A tab.
    Tab,
    /// A list item.
    ListItem,
    /// A progress bar (`value`).
    ProgressBar,
}

/// The checked state of a toggleable control (checkbox, switch, radio).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Toggled {
    /// Not applicable — the control does not toggle.
    #[default]
    None,
    /// Unchecked.
    False,
    /// Checked.
    True,
    /// **Partly** checked: some of what this stands for is on and some is not.
    ///
    /// It is a third answer rather than a second no. A "select all" that reads
    /// *unchecked* while three of its five rows are ticked is telling a reader
    /// something false, and the platform's own vocabulary has the word for it.
    Mixed,
}

/// A widget's **resolved** semantic annotation, for accessibility.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SemanticsProperties {
    /// The announced role.
    pub role: Role,
    /// Accessible name (what the screen reader reads out).
    pub label: Option<String>,
    /// Textual value (a field's contents, a slider's position, and so on).
    pub value: Option<String>,
    /// Checked state (checkboxes, switches, radios).
    pub toggled: Toggled,
    /// Actionable by click — the screen reader offers "activate".
    pub clickable: bool,
    /// Disabled (greyed out, non-interactive).
    pub disabled: bool,
    /// Numeric bounds `(min, value, max)` for sliders and progress bars.
    pub range: Option<(f32, f32, f32)>,
    /// The value is **hidden** from sight — a password field's dots — so a screen reader
    /// must not read it out either: it is told the field is a password and given only as
    /// many dots as there are characters.
    pub obscured: bool,
}

impl SemanticsProperties {
    /// An annotation carrying the given role and nothing else.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            ..Default::default()
        }
    }

    /// Sets the accessible name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the textual value.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Marks the checked state.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = if on { Toggled::True } else { Toggled::False };
        self
    }

    /// Marks it on, off, or **partly** on — `None` being the third answer, not a
    /// missing one.
    pub fn maybe_toggled(mut self, on: Option<bool>) -> Self {
        self.toggled = match on {
            Some(true) => Toggled::True,
            Some(false) => Toggled::False,
            None => Toggled::Mixed,
        };
        self
    }

    /// Marks the element as actionable.
    pub fn clickable(mut self) -> Self {
        self.clickable = true;
        self
    }

    /// Marks the element as disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the numeric bounds `(min, value, max)`.
    pub fn range(mut self, min: f32, value: f32, max: f32) -> Self {
        self.range = Some((min, value, max));
        self
    }

    /// Marks the value as hidden (a password field's).
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let field = SemanticsProperties::new(Role::TextInput).value("hunter2").obscured(true);
    /// assert!(field.obscured);
    /// ```
    pub fn obscured(mut self, obscured: bool) -> Self {
        self.obscured = obscured;
        self
    }

    /// The value as an assistive technology may read it out: the value itself, or, when it
    /// is [`obscured`](Self::obscured), one dot per character, so the length is known and
    /// the characters are not.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let field = SemanticsProperties::new(Role::TextInput).value("abc").obscured(true);
    /// assert_eq!(field.readable_value().as_deref(), Some("\u{2022}\u{2022}\u{2022}"));
    /// ```
    pub fn readable_value(&self) -> Option<String> {
        let value = self.value.as_ref()?;
        Some(if self.obscured {
            "\u{2022}".repeat(value.chars().count())
        } else {
            value.clone()
        })
    }

    /// `true` when this node carries something useful to assistive technology — a
    /// non-null role or a label. Empty containers are left out of the tree.
    pub fn is_meaningful(&self) -> bool {
        self.role != Role::None || self.label.is_some()
    }

    /// This annotation laid **over** `under`, whose answers show through wherever this
    /// one said nothing.
    ///
    /// What a container states about its child usually is not everything there is to
    /// say: something that marks a widget as a heading knows the *role* and has no idea
    /// what the words are. Dropping the child's answer would replace a spoken title with
    /// silence, which is a worse outcome than the unlabelled heading it was fixing.
    ///
    /// **Two labels are joined, not chosen between** — one line each, as the reference
    /// joins the labels of a merged subtree. A row holding a checkbox and its caption
    /// reads as one control with both, and picking one would drop the other on the floor
    /// with nothing to say which.
    ///
    /// Every other field is *this one's answer, or the other's where it had none*, with
    /// each field's "none" being its own: `Role::None`, `Toggled::None`, a `false`
    /// flag, an absent `Option`. `clickable` and `disabled` are unions rather than
    /// choices, because either one being true is a fact about the whole.
    #[must_use]
    pub fn over(self, under: &Self) -> Self {
        Self {
            role: if self.role != Role::None {
                self.role
            } else {
                under.role
            },
            label: match (self.label, under.label.clone()) {
                (Some(mine), Some(theirs)) => Some(format!("{mine}\n{theirs}")),
                (mine, theirs) => mine.or(theirs),
            },
            value: self.value.or_else(|| under.value.clone()),
            toggled: if self.toggled != Toggled::None {
                self.toggled
            } else {
                under.toggled
            },
            clickable: self.clickable || under.clickable,
            disabled: self.disabled || under.disabled,
            range: self.range.or(under.range),
            obscured: self.obscured || under.obscured,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_compose() {
        let s = SemanticsProperties::new(Role::CheckBox)
            .label("Notifications")
            .toggled(true);
        assert_eq!(s.role, Role::CheckBox);
        assert_eq!(s.label.as_deref(), Some("Notifications"));
        assert_eq!(s.toggled, Toggled::True);
        assert!(s.is_meaningful());
    }

    /// **A hidden value is never handed over** (milestone 782): an obscured field gives its
    /// length in dots, and an open one its text.
    #[test]
    fn an_obscured_value_reads_as_dots() {
        let open = SemanticsProperties::new(Role::TextInput).value("pa55");
        assert_eq!(open.readable_value().as_deref(), Some("pa55"));
        let hidden = open.clone().obscured(true);
        assert_eq!(
            hidden.readable_value().as_deref(),
            Some("\u{2022}".repeat(4).as_str())
        );
        assert_eq!(
            SemanticsProperties::new(Role::TextInput)
                .obscured(true)
                .readable_value(),
            None
        );
        // Merged over another annotation, the field stays hidden.
        assert!(SemanticsProperties::new(Role::None).over(&hidden).obscured);
    }

    #[test]
    fn empty_is_not_meaningful() {
        assert!(!SemanticsProperties::default().is_meaningful());
        // A bare label is enough to be exposed.
        assert!(SemanticsProperties::default().label("x").is_meaningful());
    }
}
