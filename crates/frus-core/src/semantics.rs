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
    /// A window over the page that takes the reader's attention until it is closed: a
    /// command palette, a dialog (milestone 809).
    Dialog,
    /// The handle between two panels that moves the boundary between them, with the
    /// first panel's share as its range (milestone 809).
    Splitter,
    /// A hierarchy of items that open and close: a file explorer, an outline.
    Tree,
    /// An item of a [`Tree`](Role::Tree): its `expanded` state says whether it is open, and
    /// `toggled` whether it is selected.
    TreeItem,
    /// A list (milestone 848): its items are its children, each a
    /// [`ListItem`](Role::ListItem) that knows its place in it.
    List,
    /// A group of related nodes read as one unit: a card, a section.
    Group,
    /// The row of [`Tab`](Role::Tab)s that switches between pages.
    TabList,
    /// A bar of a screen's title and actions: an app bar.
    Toolbar,
    /// The destinations of an application: a navigation bar, a rail, a drawer.
    Navigation,
    /// An area whose content scrolls; its [`SemanticsScroll`] says where it rests.
    ScrollView,
}

impl Role {
    /// Whether a node of this role **holds** the nodes of its subtree as its children
    /// (milestone 848), rather than standing beside them.
    ///
    /// A list holds its items, a dialog its content, a card its words and controls. A
    /// button, a checkbox, a text do not: what is inside them is part of them, and on the
    /// web a button's children are not even read.
    ///
    /// ```
    /// use frus_core::Role;
    ///
    /// assert!(Role::List.is_container());
    /// assert!(Role::Dialog.is_container());
    /// assert!(!Role::Button.is_container());
    /// ```
    pub fn is_container(self) -> bool {
        matches!(
            self,
            Role::List
                | Role::ListItem
                | Role::Group
                | Role::TabList
                | Role::Toolbar
                | Role::Navigation
                | Role::ScrollView
                | Role::Dialog
                | Role::Tree
        )
    }

    /// The role of the items a container of this role numbers (`3 of 5`), if it numbers
    /// any: a list's items, a tab bar's tabs.
    pub fn item_role(self) -> Option<Role> {
        match self {
            Role::List => Some(Role::ListItem),
            Role::TabList => Some(Role::Tab),
            _ => None,
        }
    }
}

/// Where a scrolling area rests (milestone 848): how far it is scrolled and how far it can
/// be, across and down, in the layout's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SemanticsScroll {
    /// The offset, `(x, y)`.
    pub offset: (f32, f32),
    /// The largest offset, `(x, y)`: zero on an axis that does not scroll.
    pub max: (f32, f32),
}

impl SemanticsScroll {
    /// Whether it scrolls across rather than down.
    pub fn horizontal(&self) -> bool {
        self.max.1 <= 0.5 && self.max.0 > 0.5
    }

    /// The offset and the largest offset along the axis it scrolls on.
    pub fn along(&self) -> (f32, f32) {
        if self.horizontal() {
            (self.offset.0, self.max.0)
        } else {
            (self.offset.1, self.max.1)
        }
    }

    /// Whether it can move forward (down, or across) from where it rests.
    pub fn can_forward(&self) -> bool {
        let (offset, max) = self.along();
        offset < max - 0.5
    }

    /// Whether it can move back from where it rests.
    pub fn can_backward(&self) -> bool {
        self.along().0 > 0.5
    }

    /// Where a reader's "scroll a page" takes it from where it rests, in a viewport of
    /// `viewport` (`width`, `height`): one viewport further, forward or back, along the axis
    /// it scrolls, never past its ends.
    ///
    /// ```
    /// use frus_core::SemanticsScroll;
    ///
    /// let list = SemanticsScroll { offset: (0.0, 100.0), max: (0.0, 500.0) };
    /// assert_eq!(list.paged((320.0, 300.0), true), (0.0, 400.0));
    /// assert_eq!(list.paged((320.0, 300.0), false), (0.0, 0.0));
    /// ```
    pub fn paged(&self, viewport: (f32, f32), forward: bool) -> (f32, f32) {
        let sign = if forward { 1.0 } else { -1.0 };
        if self.horizontal() {
            (
                (self.offset.0 + sign * viewport.0).clamp(0.0, self.max.0.max(0.0)),
                self.offset.1,
            )
        } else {
            (
                self.offset.0,
                (self.offset.1 + sign * viewport.1).clamp(0.0, self.max.1.max(0.0)),
            )
        }
    }
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
    /// Whether what this control opens — a menu, a sheet, a list of actions — is
    /// **showing**: `Some(true)` expanded, `Some(false)` collapsed, `None` for a control
    /// that opens nothing. A reader says "expanded" or "collapsed" after the name.
    pub expanded: Option<bool>,
    /// A **live region** (milestone 810): when the node's label or value changes while it
    /// is on screen, or the node appears, an assistive technology reads the new words out
    /// without the reader moving to it. A field's error message is one: it appears after
    /// the reader typed, somewhere they are not looking.
    pub live: bool,
    /// **Named actions** beyond activating the node (milestone 810), in the order they are
    /// offered: a list row's *Archive* and *Delete* that a finger reaches by swiping. A
    /// screen reader lists them in its actions menu; performing one sends the widget's
    /// `on_custom_action` for its index.
    pub custom_actions: Vec<String>,
    /// The **level** (milestone 848): a heading's, `1` the topmost, or a tree item's depth,
    /// `1` at the root. `None` says nothing about it.
    pub level: Option<u32>,
    /// The node's **place among its siblings** (milestone 848), `(position, count)` with the
    /// position counted from `1`: the third of five items of a list reads "3 of 5". A list
    /// numbers its items itself where they say nothing.
    pub position: Option<(usize, usize)>,
    /// Where a **scrolling area** rests (milestone 848), for a reader to say how far down it
    /// is and to scroll it.
    pub scroll: Option<SemanticsScroll>,
    /// A container whose items can be **selected several at a time** (milestone 855): a tree
    /// or a list with multiple selection. A reader says so on entering it.
    pub multiselectable: bool,
}

impl SemanticsProperties {
    /// An annotation carrying the given role and nothing else.
    pub fn new(role: Role) -> Self {
        Self {
            role,
            ..Default::default()
        }
    }

    /// Says the items inside can be selected several at a time (milestone 855).
    pub fn multiselectable(mut self, multi: bool) -> Self {
        self.multiselectable = multi;
        self
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

    /// Marks a control that opens something as **expanded** (`true`) or **collapsed**.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let toggle = SemanticsProperties::new(Role::Button).label("More").expanded(false);
    /// assert_eq!(toggle.expanded, Some(false));
    /// ```
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// Marks the node as a **live region**: a change to its words is read out where it
    /// happens.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let error = SemanticsProperties::new(Role::Label).label("Required").live(true);
    /// assert!(error.live);
    /// ```
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    /// Adds a **named action** a screen reader offers beside activating the node.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let row = SemanticsProperties::new(Role::ListItem)
    ///     .custom_action("Archive")
    ///     .custom_action("Delete");
    /// assert_eq!(row.custom_actions, ["Archive", "Delete"]);
    /// ```
    pub fn custom_action(mut self, name: impl Into<String>) -> Self {
        self.custom_actions.push(name.into());
        self
    }

    /// Sets the **level**: a heading's (`1` the topmost) or a tree item's depth.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let title = SemanticsProperties::new(Role::Heading).label("Settings").level(2);
    /// assert_eq!(title.level, Some(2));
    /// ```
    pub fn level(mut self, level: u32) -> Self {
        self.level = Some(level.max(1));
        self
    }

    /// Sets the node's place among its siblings: `position` counted from `1`, of `count`.
    ///
    /// ```
    /// use frus_core::{Role, SemanticsProperties};
    ///
    /// let row = SemanticsProperties::new(Role::ListItem).position(3, 5);
    /// assert_eq!(row.position, Some((3, 5)));
    /// ```
    pub fn position(mut self, position: usize, count: usize) -> Self {
        let position = position.max(1);
        self.position = Some((position, count.max(position)));
        self
    }

    /// Sets where a scrolling area rests.
    pub fn scroll(mut self, scroll: SemanticsScroll) -> Self {
        self.scroll = Some(scroll);
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
        self.role != Role::None || self.label.is_some() || !self.custom_actions.is_empty()
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
            expanded: self.expanded.or(under.expanded),
            live: self.live || under.live,
            custom_actions: if self.custom_actions.is_empty() {
                under.custom_actions.clone()
            } else {
                self.custom_actions
            },
            level: self.level.or(under.level),
            position: self.position.or(under.position),
            scroll: self.scroll.or(under.scroll),
            multiselectable: self.multiselectable || under.multiselectable,
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

    /// **The structure of a tree** (milestone 848): the roles that hold their subtree, the
    /// items a container numbers, and what merging keeps of a level and a place.
    #[test]
    fn containers_hold_and_number_their_items() {
        for role in [Role::List, Role::Group, Role::Dialog, Role::ScrollView] {
            assert!(role.is_container(), "{role:?}");
        }
        for role in [Role::Button, Role::Label, Role::Heading, Role::TreeItem] {
            assert!(!role.is_container(), "{role:?}");
        }
        assert_eq!(Role::List.item_role(), Some(Role::ListItem));
        assert_eq!(Role::TabList.item_role(), Some(Role::Tab));
        assert_eq!(Role::Group.item_role(), None);
        let merged = SemanticsProperties::new(Role::None).over(
            &SemanticsProperties::new(Role::Heading)
                .level(2)
                .position(1, 3),
        );
        assert_eq!(merged.level, Some(2));
        assert_eq!(merged.position, Some((1, 3)));
        let down = SemanticsScroll {
            offset: (0.0, 40.0),
            max: (0.0, 300.0),
        };
        assert!(!down.horizontal());
        assert!(down.can_forward() && down.can_backward());
        let across = SemanticsScroll {
            offset: (0.0, 0.0),
            max: (120.0, 0.0),
        };
        assert!(across.horizontal() && across.can_forward() && !across.can_backward());
    }

    #[test]
    fn empty_is_not_meaningful() {
        assert!(!SemanticsProperties::default().is_meaningful());
        // A bare label is enough to be exposed.
        assert!(SemanticsProperties::default().label("x").is_meaningful());
    }
}
