// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Semantics`]: states what a widget **is**, from outside the widget.
//!
//! Every other widget in this crate answers [`Widget::semantics`] for *itself*. That
//! covers the ordinary case and misses the one this exists for: a caller is handed a
//! built widget and knows something about it that the widget does not know about itself.
//!
//! Milestone 397 walked into it. An [`AppBar`] marks its **text** title as
//! a heading — a landmark a screen reader's user jumps between — and could do nothing of
//! the sort for a *widget* title, because by then the title is a `Box<dyn Widget>` and the
//! bar has no way in. The accessibility of a bar therefore depended on whether the caller
//! had passed a string or a widget, which is not a distinction anybody using assistive
//! technology should be able to feel.

use frus_core::{Rect, Role, Scene, SemanticsProperties};
use frus_layout::Style;

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// What a [`Semantics`] wrapper asks the walk to do with its subtree's annotations.
///
/// Returned by [`Widget::describes`] and read in one place, the way a
/// [`ModalBarrier`](crate::barrier::ModalBarrier) is: the subtree is walked exactly as
/// usual and what it produced is reconciled afterwards. Doing it after rather than instead
/// is what makes it exact — a widget deep inside annotates itself without knowing anything
/// is speaking for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Description {
    /// What the wrapper says about its child.
    pub props: SemanticsProperties,
    /// Replace the subtree's own annotations with this one instead of adding to them.
    pub merging: bool,
}

/// States a **role, a name or a state** for a child that cannot state it for itself — the
/// reference's `Semantics` widget.
///
/// ```ignore
/// // A widget title announced as a landmark rather than as one more run of prose.
/// Semantics::heading(my_title_widget)
///
/// // A row of two controls that reads as one thing.
/// Semantics::merge(Flex::row().child(Checkbox::new(on)).child(Text::new("Notifications")))
/// ```
///
/// # It adds a node; merging replaces them
///
/// By default this **adds** an annotation and leaves the child's alone: safe, and what a
/// caller naming an otherwise anonymous box wants. [`Semantics::merging`] is the other
/// answer — the subtree's annotations are dropped and this one speaks for all of them,
/// carrying over what they said through [`SemanticsProperties::over`].
///
/// The default is the additive one **because the destructive one cannot be undone by the
/// caller**. Wrapping a whole screen to name it, under a merging default, would collapse
/// every control on it into a single node; the reverse mistake leaves one node too many,
/// which is noise rather than loss.
///
/// The reference splits the same two behaviours across `Semantics(excludeSemantics:)` and
/// a separate `MergeSemantics`; [`Semantics::merge`] is that second widget, spelled as the
/// constructor it is.
///
/// # It holds what it describes
///
/// Since milestone 848 the accessibility tree has a shape. A wrapper that says what a whole
/// subtree **is** — a role that holds others (a list, a group, a dialog), or no role and a
/// name, a section — becomes the subtree's parent node, and a reader can step into it or
/// over it as a unit. One that names a single control (a button, a heading) stands beside
/// what it annotates, as it always did. Merging joins two labels into one string, one line
/// each, which is what the reference does with a merged subtree too.
pub struct Semantics<Msg = crate::callback::Callback> {
    props: SemanticsProperties,
    merging: bool,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg> Semantics<Msg> {
    /// Annotates `child` with `props`, adding a node and leaving the child's own alone.
    pub fn new(props: SemanticsProperties, child: impl Widget<Msg> + 'static) -> Self {
        Self {
            props,
            merging: false,
            children: vec![Box::new(child)],
        }
    }

    /// Announces `child` as a **heading**: a landmark assistive technology can jump
    /// between, rather than one more run of prose.
    ///
    /// It merges, and that is the point of the shorthand. A heading that also announced
    /// the text inside it as a separate label would be read twice — once as a landmark and
    /// once as prose — so the wrapper takes the child's words and speaks for it.
    pub fn heading(child: impl Widget<Msg> + 'static) -> Self {
        Self::new(SemanticsProperties::new(Role::Heading), child).merging(true)
    }

    /// Makes `child`'s whole subtree read as **one thing** — the reference's
    /// `MergeSemantics`, for a row of controls that is really a single control.
    ///
    /// It states nothing of its own: everything announced comes from the subtree, joined.
    pub fn merge(child: impl Widget<Msg> + 'static) -> Self {
        Self::new(SemanticsProperties::default(), child).merging(true)
    }

    /// Whether this speaks **for** the subtree (dropping its annotations and carrying over
    /// what they said) or merely **beside** it. Off by default; see the type's docs for
    /// why that is the safe default rather than the faithful one.
    pub fn merging(mut self, merging: bool) -> Self {
        self.merging = merging;
        self
    }

    /// Sets the accessible name — what a screen reader reads out.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.props.label = Some(label.into());
        self
    }

    /// Sets the textual value: a field's contents, a slider's position.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.props.value = Some(value.into());
        self
    }

    /// Sets the announced role.
    pub fn role(mut self, role: Role) -> Self {
        self.props.role = role;
        self
    }

    /// Marks it checked or unchecked.
    pub fn toggled(mut self, on: bool) -> Self {
        self.props = self.props.toggled(on);
        self
    }

    /// Announces it as actionable — the reader offers "activate".
    pub fn clickable(mut self, clickable: bool) -> Self {
        self.props.clickable = clickable;
        self
    }

    /// Announces it as disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.props.disabled = disabled;
        self
    }

    /// Sets the level: a heading's, `1` the topmost, or a tree item's depth (milestone 848).
    pub fn level(mut self, level: u32) -> Self {
        self.props = self.props.level(level);
        self
    }

    /// Sets its place among its siblings: `position` counted from `1`, of `count`
    /// (milestone 848). A list numbers the items that say nothing of theirs itself.
    pub fn position(mut self, position: usize, count: usize) -> Self {
        self.props = self.props.position(position, count);
        self
    }
}

impl<Msg: Clone> Widget<Msg> for Semantics<Msg> {
    /// A **transparent** box: the child's own style decides everything, and this one is
    /// the pass-through a single-child wrapper has to be to change nothing that is drawn.
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    /// A heading's words are a heading on the page too (milestone 833).
    fn page_heading(&self) -> bool {
        self.props.role == Role::Heading
    }

    /// It states what its child **is**, never what it does: an annotation that swallowed a
    /// click would put a hit target over the widget it was describing.
    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn describes(&self) -> Option<Description> {
        Some(Description {
            props: self.props.clone(),
            merging: self.merging,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Button, Flex, Runtime, Text, Ui};
    use frus_core::Size as CoreSize;

    fn built(root: &dyn Widget<()>) -> Ui<()> {
        build_ui(
            root,
            CoreSize::new(300.0, 200.0),
            &Runtime::default(),
            &Theme::default(),
        )
    }

    /// Every annotation the built tree carries, in walk order.
    fn described(root: &dyn Widget<()>) -> Vec<(Role, Option<String>)> {
        built(root)
            .semantics()
            .iter()
            .map(|(_, _, s)| (s.role, s.label.clone()))
            .collect()
    }

    /// **A caller can mark an assembled widget as a heading**, which is the gap milestone
    /// 397 left: an app bar could do it for a text title and not for a widget one.
    ///
    /// And it keeps the words. A heading with no label is a landmark that announces
    /// nothing on arrival — worse than the unmarked text it replaced, not better — so the
    /// wrapper states the role and takes the label from what it wrapped.
    #[test]
    fn a_heading_takes_the_role_it_states_and_the_words_it_wrapped() {
        let root: Box<dyn Widget<()>> = Box::new(Semantics::heading(Text::new("Settings")));
        assert_eq!(
            described(root.as_ref()),
            vec![(Role::Heading, Some("Settings".to_string()))],
            "one node: a heading that says the words"
        );
    }

    /// Left alone it **adds**, and the child keeps its own voice.
    ///
    /// This is the half that makes the default safe. A caller naming a box gets a name and
    /// still has everything inside it; a merging default would have silently collapsed a
    /// whole screen into one node the first time somebody named one.
    #[test]
    fn without_merging_the_child_still_speaks() {
        let root: Box<dyn Widget<()>> = Box::new(
            Semantics::new(SemanticsProperties::default(), Text::new("Settings"))
                .label("A section"),
        );
        let found = described(root.as_ref());
        assert_eq!(found.len(), 2, "two nodes, not one: {found:?}");
        assert!(found.contains(&(Role::Label, Some("Settings".to_string()))));
        assert!(found.iter().any(|(_, l)| l.as_deref() == Some("A section")));
    }

    /// The built tree as `(role, label, depth)`, depth counted from `0` at the top
    /// (milestone 848).
    fn shaped(root: &dyn Widget<()>) -> Vec<(Role, Option<String>, usize)> {
        let ui = built(root);
        let parents = ui.semantics_parents();
        let depth = |mut at: usize| {
            let mut depth = 0;
            while let Some(parent) = parents[at] {
                depth += 1;
                at = parent;
            }
            depth
        };
        ui.semantics()
            .iter()
            .enumerate()
            .map(|(i, (_, _, s))| (s.role, s.label.clone(), depth(i)))
            .collect()
    }

    /// **A section holds what it names** (milestone 848): the wrapper that names a box is
    /// the parent of the box's own nodes, which a reader steps into as a unit.
    #[test]
    fn a_named_section_holds_its_content() {
        let root: Box<dyn Widget<()>> = Box::new(
            Semantics::new(
                SemanticsProperties::default(),
                Flex::column()
                    .child(Text::new("Wi-Fi"))
                    .child(Button::new("Connect").on_press(())),
            )
            .label("Network"),
        );
        let found = shaped(root.as_ref());
        assert_eq!(
            found,
            vec![
                (Role::None, Some("Network".to_string()), 0),
                (Role::Label, Some("Wi-Fi".to_string()), 1),
                (Role::Button, Some("Connect".to_string()), 1),
            ]
        );
    }

    /// **A list holds its items, and each knows its place** among all of them, built or not
    /// (milestone 848): a row of a checkbox and its caption is an item holding both.
    #[test]
    fn a_list_holds_numbered_items() {
        let root: Box<dyn Widget<()>> = Box::new(
            crate::ListView::new(50, 40.0, |i| {
                Flex::row()
                    .child(crate::Checkbox::new(i % 2 == 0).on_toggle(|_| ()))
                    .child(Text::new(format!("Task {i}")))
            })
            .height(120.0),
        );
        let ui = built(root.as_ref());
        let nodes = ui.semantics();
        let parents = ui.semantics_parents();
        let (_, _, list) = &nodes[0];
        assert_eq!(list.role, Role::List);
        let scroll = list.scroll.expect("a list says where it rests");
        assert_eq!(scroll.offset, (0.0, 0.0));
        assert!(scroll.max.1 > 1000.0, "fifty rows of 40 in 120: {scroll:?}");
        let items: Vec<usize> = (0..nodes.len())
            .filter(|&i| parents[i] == Some(0))
            .collect();
        assert!(items.len() >= 3, "the rows in view: {nodes:?}");
        for (n, &item) in items.iter().enumerate() {
            assert_eq!(nodes[item].2.role, Role::ListItem);
            assert_eq!(nodes[item].2.position, Some((n + 1, 50)));
        }
        // The first item holds its checkbox and its caption.
        let held: Vec<Role> = (0..nodes.len())
            .filter(|&i| parents[i] == Some(items[0]))
            .map(|i| nodes[i].2.role)
            .collect();
        assert_eq!(held, vec![Role::CheckBox, Role::Label]);

        // Told it is not a list, it is only an area that scrolls.
        let page: Box<dyn Widget<()>> = Box::new(
            crate::ListView::new(50, 40.0, |i| Text::new(format!("Line {i}")))
                .height(120.0)
                .semantic_list(false),
        );
        let found = shaped(page.as_ref());
        assert_eq!(found[0].0, Role::ScrollView);
        assert!(found[1..]
            .iter()
            .all(|(role, _, depth)| *role == Role::Label && *depth == 1));
    }

    /// **A card is one unit** (milestone 848) holding its words and controls; told not to be,
    /// it adds nothing; pressed as a whole, its words name the press.
    #[test]
    fn a_card_groups_what_it_holds() {
        let card = || {
            crate::Card::new().child(
                Flex::column()
                    .child(Text::new("Order 42"))
                    .child(Button::new("Track").on_press(())),
            )
        };
        let root: Box<dyn Widget<()>> = Box::new(card());
        assert_eq!(
            shaped(root.as_ref()),
            vec![
                (Role::Group, None, 0),
                (Role::Label, Some("Order 42".to_string()), 1),
                (Role::Button, Some("Track".to_string()), 1),
            ]
        );
        let flat: Box<dyn Widget<()>> = Box::new(card().semantic_container(false));
        assert!(shaped(flat.as_ref())
            .iter()
            .all(|(_, _, depth)| *depth == 0));
        assert_eq!(shaped(flat.as_ref()).len(), 2);

        let pressed: Box<dyn Widget<()>> = Box::new(
            crate::GestureDetector::new(crate::Card::new().child(Text::new("Order 42"))).on_tap(()),
        );
        assert_eq!(
            shaped(pressed.as_ref()),
            vec![(Role::Button, Some("Order 42".to_string()), 0)],
            "a pressed card reads as the button its words name, as before cards were groups"
        );
    }

    /// **A heading says its level** (milestone 848), from a text or from a wrapper.
    #[test]
    fn a_heading_carries_its_level() {
        let root: Box<dyn Widget<()>> = Box::new(
            Flex::column()
                .child(Text::new("Account").heading_level(1))
                .child(Semantics::heading(Text::new("Security")).level(2)),
        );
        let levels: Vec<(Role, Option<u32>)> = built(root.as_ref())
            .semantics()
            .iter()
            .map(|(_, _, s)| (s.role, s.level))
            .collect();
        assert_eq!(
            levels,
            vec![(Role::Heading, Some(1)), (Role::Heading, Some(2))]
        );
    }

    /// **A scroll view says where it rests** (milestone 848) and holds what scrolls in it; one
    /// whose content fits adds nothing.
    #[test]
    fn a_scroll_view_holds_its_content_when_it_scrolls() {
        let tall = || {
            let mut column = Flex::column();
            for i in 0..20 {
                column =
                    column.child(crate::SizedBox::new(Text::new(format!("Row {i}"))).height(40.0));
            }
            column
        };
        let root: Box<dyn Widget<()>> = Box::new(
            crate::SingleChildScrollView::new()
                .height(100.0)
                .child(tall()),
        );
        let found = shaped(root.as_ref());
        assert_eq!(found[0].0, Role::ScrollView);
        assert!(found[1..].iter().all(|(_, _, depth)| *depth == 1));
        let fits: Box<dyn Widget<()>> = Box::new(
            crate::SingleChildScrollView::new()
                .height(100.0)
                .child(Text::new("Short")),
        );
        assert_eq!(
            shaped(fits.as_ref()),
            vec![(Role::Label, Some("Short".to_string()), 0)]
        );
    }

    /// **Two labels are joined, not chosen between.** A row of a control and its caption
    /// reads as one thing with both; picking one would drop the other with nothing to say
    /// which was lost.
    #[test]
    fn merging_joins_the_labels_it_swallows() {
        let root: Box<dyn Widget<()>> = Box::new(Semantics::merge(
            Flex::row()
                .child(Text::new("Notifications"))
                .child(Text::new("On")),
        ));
        let found = described(root.as_ref());
        assert_eq!(found.len(), 1, "one node: {found:?}");
        assert_eq!(found[0].1.as_deref(), Some("Notifications\nOn"));
    }

    /// **A merged control stays operable.** Found testing #18 in a real browser and a
    /// real screen reader: `Semantics::merge` keyed the surviving node on the
    /// *wrapper's* id, which nothing routes a click through, so a button merged with
    /// its own caption announced correctly and did nothing on activation — on every
    /// platform, AccessKit desktop included, since the bug was here rather than in
    /// either bridge. The merged node now carries the id of the one child that was
    /// clickable, so a click, a keyboard `Enter`, or an assistive technology's own
    /// activation all still reach it.
    #[test]
    fn merging_keeps_the_clickable_childs_id_so_it_can_still_be_activated() {
        let root: Box<dyn Widget<()>> = Box::new(Semantics::merge(
            Flex::row()
                .child(Button::new("Save").on_press(()))
                .child(Text::new("your changes")),
        ));
        let ui = built(root.as_ref());
        let found = ui.semantics();
        assert_eq!(found.len(), 1, "one node: {found:?}");
        let (id, _, props) = &found[0];
        assert_eq!(props.label.as_deref(), Some("Save\nyour changes"));
        assert_eq!(
            ui.msg_for(*id),
            Some(()),
            "the merged node's id is the button's own — the one thing in the \
             subtree a click, Enter, or an AT's activation can actually be routed to"
        );
    }

    /// **Two clickable children have no single answer**, so nothing is guessed: the
    /// merged node falls back to the wrapper's own id, exactly as before this fix —
    /// silently picking one of two buttons would be its own, quieter bug.
    #[test]
    fn merging_two_clickable_children_falls_back_to_the_wrappers_id() {
        let root: Box<dyn Widget<()>> = Box::new(Semantics::merge(
            Flex::row()
                .child(Button::new("Save").on_press(()))
                .child(Button::new("Cancel").on_press(())),
        ));
        let ui = built(root.as_ref());
        let found = ui.semantics();
        assert_eq!(found.len(), 1, "one node: {found:?}");
        assert_eq!(ui.msg_for(found[0].0), None);
    }

    /// An annotation that says nothing and swallows nothing puts **no node** in the tree.
    ///
    /// The same rule every other widget is held to — `is_meaningful` — applied to the one
    /// widget whose whole purpose is to add a node. A wrapper that announced an empty
    /// container would be one more thing for a reader to step through on the way to the
    /// content.
    #[test]
    fn an_empty_annotation_adds_nothing() {
        let root: Box<dyn Widget<()>> = Box::new(Semantics::merge(crate::Container::new()));
        assert!(described(root.as_ref()).is_empty());
    }
}
