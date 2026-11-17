// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`FileTree`]: folders and files, opened and closed in place — a project's explorer, a
//! document outline, a settings hierarchy (milestone 811).
//!
//! The application holds the nodes, which ones are open and which one is selected; the
//! tree shows the open part of the hierarchy, one row per visible node, and says what was
//! asked of it. A folder whose children are not known yet is a **lazy** folder: opening it
//! asks the application to load them, and a loading row stands under it until they come.
//!
//! The keyboard works as in a desktop explorer: Up and Down go from row to row, Right
//! opens a closed folder, Left closes an open one, Enter and Space select. A screen reader
//! hears a tree of items, each with its name and, for a folder, whether it is open.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::rc::Rc;

use frus_core::{Color, Rect, Role, Scene, SemanticsProperties, TextStyle};
use frus_layout::{Align, Dimension, FlexDirection, Style};

use crate::flex::Flex;
use crate::icons::Icons;
use crate::interaction::{Key, KeyResponse, Status};
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::Widget;

/// Default height of a row.
pub const FILE_TREE_ROW_HEIGHT: f32 = 28.0;
/// Default indentation per level.
pub const FILE_TREE_INDENT: f32 = 16.0;

/// Theme defaults for [`FileTree`] (`theme.widgets.file_tree`); each `None` falls through
/// to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FileTreeTheme {
    /// A row's height. Unset, 28.
    pub row_height: Option<f32>,
    /// The indentation per level. Unset, 16.
    pub indent: Option<f32>,
    /// The names' style. Unset, `body_medium`.
    pub text_style: Option<TextStyle>,
    /// The names' colour. Unset, `on_surface`.
    pub text_color: Option<Color>,
    /// The selected row's background. Unset, `secondary_container`.
    pub selected_color: Option<Color>,
    /// The selected row's name. Unset, `on_secondary_container`.
    pub selected_text_color: Option<Color>,
    /// A folder's icon. Unset, `primary`.
    pub folder_color: Option<Color>,
    /// A file's icon and the chevrons. Unset, `on_surface_variant`.
    pub icon_color: Option<Color>,
    /// The icons' size. Unset, 18.
    pub icon_size: Option<f32>,
}

/// What a node is: a file, or a folder whose children are known or not yet.
#[derive(Clone, Debug, PartialEq)]
pub enum FileNodeKind {
    /// A leaf.
    File,
    /// A folder. `None`: its children have not been loaded.
    Folder(Option<Vec<FileNode>>),
}

/// One node of a [`FileTree`].
#[derive(Clone, Debug, PartialEq)]
pub struct FileNode {
    /// What identifies the node in the tree's messages. Unique across the tree.
    pub id: u64,
    /// Its name.
    pub label: String,
    /// File or folder.
    pub kind: FileNodeKind,
}

impl FileNode {
    /// A file.
    pub fn file(id: u64, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FileNodeKind::File,
        }
    }

    /// A folder holding `children`.
    pub fn folder(
        id: u64,
        label: impl Into<String>,
        children: impl IntoIterator<Item = FileNode>,
    ) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FileNodeKind::Folder(Some(children.into_iter().collect())),
        }
    }

    /// A folder whose children are **not loaded yet**: opening it shows a loading row until
    /// the application gives it children.
    pub fn lazy_folder(id: u64, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            kind: FileNodeKind::Folder(None),
        }
    }

    /// Whether the node is a folder.
    pub fn is_folder(&self) -> bool {
        matches!(self.kind, FileNodeKind::Folder(_))
    }
}

type IconBuilder<Msg> = Rc<dyn Fn(&FileNode, bool) -> Box<dyn Widget<Msg>>>;

/// One row as the tree lays it out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Line<'a> {
    Node {
        node: &'a FileNode,
        depth: usize,
        open: bool,
    },
    Loading {
        depth: usize,
    },
}

/// The rows the open part of `nodes` makes, in reading order.
fn visible<'a>(
    nodes: &'a [FileNode],
    expanded: &HashSet<u64>,
    depth: usize,
    out: &mut Vec<Line<'a>>,
) {
    for node in nodes {
        let open = node.is_folder() && expanded.contains(&node.id);
        out.push(Line::Node { node, depth, open });
        if open {
            match &node.kind {
                FileNodeKind::Folder(Some(children)) => visible(children, expanded, depth + 1, out),
                FileNodeKind::Folder(None) => out.push(Line::Loading { depth: depth + 1 }),
                FileNodeKind::File => {}
            }
        }
    }
}

/// A **tree of folders and files**, opened and closed in place.
///
/// ```
/// use frus_widgets::{FileNode, FileTree};
///
/// #[derive(Clone)]
/// enum Msg { Toggle(u64, bool), Select(u64) }
///
/// let _tree = FileTree::new(vec![
///     FileNode::folder(1, "src", [FileNode::file(2, "main.rs"), FileNode::lazy_folder(3, "widgets")]),
///     FileNode::file(4, "Cargo.toml"),
/// ])
/// .expanded([1])
/// .selected(Some(2))
/// .on_toggle(Msg::Toggle)
/// .on_select(Msg::Select);
/// ```
///
/// `on_toggle(id, open)` asks to open or close a folder — the chevron, Right or Left, or a
/// click on a folder's row when nothing is selectable; `on_select(id)` a row's body, Enter
/// or Space. The icons can be replaced ([`FileTree::icon`]). **Themed** through
/// [`FileTreeTheme`] (`theme.widgets.file_tree`).
pub struct FileTree<Msg = crate::callback::Callback> {
    roots: Vec<FileNode>,
    expanded: HashSet<u64>,
    selected: Option<u64>,
    on_toggle: Option<Rc<dyn Fn(u64, bool) -> Msg>>,
    on_select: Option<Rc<dyn Fn(u64) -> Msg>>,
    icon: Option<IconBuilder<Msg>>,
    label: Option<String>,
    loading_label: String,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> FileTree<Msg> {
    /// A tree of `roots`, every folder closed.
    pub fn new(roots: impl IntoIterator<Item = FileNode>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
            expanded: HashSet::new(),
            selected: None,
            on_toggle: None,
            on_select: None,
            icon: None,
            label: None,
            loading_label: crate::localizations::of().loading_label().into(),
            built: OnceCell::new(),
        }
    }

    /// The folders that are open.
    pub fn expanded(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.expanded = ids.into_iter().collect();
        self
    }

    /// The selected node, highlighted.
    pub fn selected(mut self, id: Option<u64>) -> Self {
        self.selected = id;
        self
    }

    /// `on_toggle(id, open)`: a folder is to be opened (`true`) or closed.
    pub fn on_toggle<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_toggle: impl Fn(u64, bool) -> R + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(crate::callback::handler2(on_toggle)));
        self
    }

    /// `on_select(id)`: a node is to be selected.
    pub fn on_select<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_select: impl Fn(u64) -> R + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(crate::callback::handler1(on_select)));
        self
    }

    /// The icon of each row, in place of the folder and file glyphs: `icon(node, open)`.
    pub fn icon<W: Widget<Msg> + 'static>(
        mut self,
        icon: impl Fn(&FileNode, bool) -> W + 'static,
    ) -> Self {
        self.icon = Some(Rc::new(move |node, open| {
            Box::new(icon(node, open)) as Box<dyn Widget<Msg>>
        }));
        self
    }

    /// What a screen reader calls the tree.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The words of the row under a folder still loading.
    pub fn loading_label(mut self, label: impl Into<String>) -> Self {
        self.loading_label = label.into();
        self
    }

    /// The rows, built under `theme`.
    fn build(&self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let t = theme.widgets.file_tree;
        let row_height = t.row_height.unwrap_or(FILE_TREE_ROW_HEIGHT);
        let indent = t.indent.unwrap_or(FILE_TREE_INDENT);
        let icon_size = t.icon_size.unwrap_or(18.0);
        let icon_color = t.icon_color.unwrap_or(theme.scheme.on_surface_variant);
        let text_style = t.text_style.unwrap_or(theme.text.body_medium);
        let mut lines = Vec::new();
        visible(&self.roots, &self.expanded, 0, &mut lines);
        // Rows span the width the tree is given, so the whole row is a click target.
        let mut column = Flex::column().align(Align::Stretch).width_fraction(1.0);
        for line in lines {
            match line {
                Line::Node { node, depth, open } => {
                    let selected = self.selected == Some(node.id);
                    let fg = if selected {
                        t.selected_text_color
                            .unwrap_or(theme.scheme.on_secondary_container)
                    } else {
                        t.text_color.unwrap_or(theme.scheme.on_surface)
                    };
                    let folder = node.is_folder();
                    let toggle = if folder {
                        self.on_toggle.as_ref().map(|f| f(node.id, !open))
                    } else {
                        None
                    };
                    let mut content = Flex::row()
                        .align(Align::Center)
                        .height(row_height)
                        .child(crate::SizedBox::<Msg>::empty().width(depth as f32 * indent));
                    // The chevron: its own target, so a click on it opens or closes, and a
                    // click on the rest of the row selects.
                    let chevron: Box<dyn Widget<Msg>> = if folder {
                        let glyph = crate::Icon::new(if open {
                            Icons::EXPAND_MORE
                        } else {
                            Icons::CHEVRON_RIGHT
                        })
                        .size(icon_size)
                        .color(icon_color);
                        let boxed = crate::Container::new()
                            .width(icon_size + 6.0)
                            .height(row_height)
                            .alignment(frus_core::Alignment::CENTER)
                            .child(glyph);
                        match toggle.clone() {
                            Some(message) => {
                                Box::new(crate::GestureDetector::new(boxed).on_tap(message))
                            }
                            None => Box::new(boxed),
                        }
                    } else {
                        Box::new(crate::SizedBox::<Msg>::empty().width(icon_size + 6.0))
                    };
                    content = content.child(chevron);
                    let icon: Box<dyn Widget<Msg>> = match &self.icon {
                        Some(build) => build(node, open),
                        None => Box::new(
                            crate::Icon::new(match (folder, open) {
                                (true, true) => Icons::FOLDER_OPEN,
                                (true, false) => Icons::FOLDER,
                                _ => Icons::INSERT_DRIVE_FILE,
                            })
                            .size(icon_size)
                            .color(if folder {
                                t.folder_color.unwrap_or(theme.scheme.primary)
                            } else {
                                icon_color
                            }),
                        ),
                    };
                    content = content
                        .child(icon)
                        .child(crate::SizedBox::<Msg>::empty().width(6.0))
                        .child(crate::Expanded::new(
                            Text::styled(node.label.clone(), text_style)
                                .color(fg)
                                .no_wrap()
                                .ellipsis(),
                        ));
                    column = column.child(FileRow {
                        id: node.id,
                        label: node.label.clone(),
                        folder,
                        open,
                        selected,
                        toggle,
                        select: self.on_select.as_ref().map(|f| f(node.id)),
                        open_message: if folder && !open {
                            self.on_toggle.as_ref().map(|f| f(node.id, true))
                        } else {
                            None
                        },
                        close_message: if folder && open {
                            self.on_toggle.as_ref().map(|f| f(node.id, false))
                        } else {
                            None
                        },
                        height: row_height,
                        selected_color: t
                            .selected_color
                            .unwrap_or(theme.scheme.secondary_container),
                        children: vec![Box::new(crate::ExcludeSemantics::new(content))],
                    });
                }
                Line::Loading { depth } => {
                    column = column.child(
                        Flex::row()
                            .align(Align::Center)
                            .height(row_height)
                            .padding_each(0.0, 0.0, 0.0, depth as f32 * indent + icon_size + 6.0)
                            .child(
                                Text::styled(self.loading_label.clone(), text_style)
                                    .italic()
                                    .color(theme.scheme.on_surface_variant),
                            ),
                    );
                }
            }
        }
        let mut sem = SemanticsProperties::new(Role::Tree);
        if let Some(label) = &self.label {
            sem = sem.label(label.clone());
        }
        Box::new(crate::Semantics::new(sem, column))
    }
}

/// One node's row: a focus stop that selects, opens and closes, and paints its selection.
struct FileRow<Msg> {
    id: u64,
    label: String,
    folder: bool,
    open: bool,
    selected: bool,
    toggle: Option<Msg>,
    select: Option<Msg>,
    open_message: Option<Msg>,
    close_message: Option<Msg>,
    height: f32,
    selected_color: Color,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for FileRow<Msg> {
    fn style(&self) -> Style {
        Style {
            height: Dimension::Length(self.height),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    /// The row keeps its identity — its focus, its hover — however the rows above it change.
    fn key(&self) -> Option<u64> {
        Some(self.id)
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        if self.selected {
            scene.draw_rect(
                bounds,
                self.selected_color.fade(o),
                theme.radius,
                0.0,
                Color::TRANSPARENT,
            );
        } else if status.hover_progress > 0.0
            || status.press_progress > 0.0
            || status.focus_progress > 0.0
        {
            let bg = theme.state_layer(theme.scheme.surface, theme.scheme.on_surface, &status);
            scene.draw_rect(bounds, bg.fade(o), theme.radius, 0.0, Color::TRANSPARENT);
        }
    }

    /// A click (or Enter, or Space) selects; with nothing selectable, a folder opens or
    /// closes.
    fn on_click(&self) -> Option<Msg> {
        self.select.clone().or_else(|| self.toggle.clone())
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Right opens a closed folder, Left closes an open one; anything else goes on.
    fn on_key(&self, key: &Key) -> KeyResponse<Msg> {
        match key {
            Key::Right { .. } if self.open_message.is_some() => {
                KeyResponse::Handled(self.open_message.clone())
            }
            Key::Left { .. } if self.close_message.is_some() => {
                KeyResponse::Handled(self.close_message.clone())
            }
            _ => KeyResponse::Ignored,
        }
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        let mut sem = SemanticsProperties::new(Role::TreeItem)
            .label(self.label.clone())
            .toggled(self.selected)
            .clickable();
        if self.folder {
            sem = sem.expanded(self.open);
        }
        Some(sem)
    }

    fn debug_name(&self) -> &'static str {
        "FileTreeRow"
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for FileTree<Msg> {
    fn style(&self) -> Style {
        // The full width, stacked: the child (the tree's semantics box) stretches across it,
        // so every row is as wide as the tree.
        Style {
            width: Dimension::Percent(1.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    fn build_themed(&self, theme: &Theme) {
        let _ = self.built.get_or_init(|| vec![self.build(theme)]);
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built
            .get_or_init(|| vec![self.build(&Theme::default())])
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "FileTree"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, find_widget, Point, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Toggle(u64, bool),
        Select(u64),
    }

    fn sample() -> Vec<FileNode> {
        vec![
            FileNode::folder(
                1,
                "src",
                [
                    FileNode::file(2, "main.rs"),
                    FileNode::folder(3, "widgets", [FileNode::file(4, "tree.rs")]),
                ],
            ),
            FileNode::lazy_folder(5, "assets"),
            FileNode::file(6, "Cargo.toml"),
        ]
    }

    fn tree(expanded: &[u64]) -> FileTree<Msg> {
        FileTree::new(sample())
            .expanded(expanded.iter().copied())
            .on_toggle(Msg::Toggle)
            .on_select(Msg::Select)
            .label("Project")
    }

    fn rows(t: &FileTree<Msg>) -> Vec<(String, bool)> {
        let theme = Theme::light();
        let ui = build_ui(t, Size::new(300.0, 400.0), &Runtime::default(), &theme);
        ui.semantics()
            .iter()
            .filter(|(_, _, s)| s.role == Role::TreeItem)
            .map(|(_, _, s)| {
                (
                    s.label.clone().unwrap_or_default(),
                    s.expanded == Some(true),
                )
            })
            .collect()
    }

    #[test]
    fn only_the_open_part_is_shown() {
        let closed = rows(&tree(&[]));
        let names: Vec<&str> = closed.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(names, ["src", "assets", "Cargo.toml"]);
        let open = rows(&tree(&[1, 3]));
        let names: Vec<&str> = open.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            names,
            [
                "src",
                "main.rs",
                "widgets",
                "tree.rs",
                "assets",
                "Cargo.toml"
            ]
        );
        assert!(open[0].1, "src reads as expanded");
    }

    #[test]
    fn a_lazy_folder_opened_shows_a_loading_row() {
        let mut lines = Vec::new();
        let nodes = sample();
        visible(&nodes, &[5].into_iter().collect(), 0, &mut lines);
        assert!(lines.contains(&Line::Loading { depth: 1 }), "{lines:?}");
        let theme = Theme::light();
        let ui = build_ui(
            &tree(&[5]),
            Size::new(300.0, 400.0),
            &Runtime::default(),
            &theme,
        );
        let words: Vec<String> = ui
            .scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                frus_core::Primitive::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(words.iter().any(|w| w == "Loading…"), "{words:?}");
    }

    #[test]
    fn the_tree_and_its_items_have_their_roles() {
        let theme = Theme::light();
        let ui = build_ui(
            &tree(&[1]),
            Size::new(300.0, 400.0),
            &Runtime::default(),
            &theme,
        );
        let root = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == Role::Tree)
            .expect("a tree");
        assert_eq!(root.2.label.as_deref(), Some("Project"));
        let file = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some("main.rs"))
            .expect("main.rs");
        assert_eq!(file.2.expanded, None, "a file opens nothing");
        let names: Vec<&str> = ui
            .semantics()
            .iter()
            .filter_map(|(_, _, s)| s.label.as_deref())
            .collect();
        assert_eq!(
            names.iter().filter(|n| **n == "main.rs").count(),
            1,
            "each name is read once: {names:?}"
        );
    }

    /// A click on the chevron opens; on the rest of the row, selects.
    #[test]
    fn the_chevron_toggles_and_the_body_selects() {
        let theme = Theme::light();
        let t = tree(&[]);
        let ui = build_ui(&t, Size::new(300.0, 400.0), &Runtime::default(), &theme);
        let src = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some("src"))
            .map(|(_, r, _)| *r)
            .expect("src");
        let click = |x: f32| {
            let id = ui
                .hit(Point::new(x, src.y + src.height * 0.5))
                .expect("a target");
            find_widget(&t, id).and_then(|w| w.on_click())
        };
        assert_eq!(click(src.x + 8.0), Some(Msg::Toggle(1, true)));
        assert_eq!(click(src.x + 150.0), Some(Msg::Select(1)));
    }

    /// Right opens a closed folder, Left closes an open one; Up and Down are left to the
    /// focus, which moves from row to row.
    #[test]
    fn arrows_open_and_close() {
        let theme = Theme::light();
        let t = tree(&[1]);
        let ui = build_ui(&t, Size::new(300.0, 400.0), &Runtime::default(), &theme);
        let row = |name: &str| {
            let (id, _, _) = ui
                .semantics()
                .iter()
                .find(|(_, _, s)| s.label.as_deref() == Some(name))
                .expect("the row");
            *id
        };
        let right = Key::Right {
            shift: false,
            word: false,
        };
        let left = Key::Left {
            shift: false,
            word: false,
        };
        let src = find_widget(&t, row("src")).expect("src");
        assert_eq!(
            src.on_key(&left),
            KeyResponse::Handled(Some(Msg::Toggle(1, false)))
        );
        assert_eq!(src.on_key(&right), KeyResponse::Ignored);
        let widgets = find_widget(&t, row("widgets")).expect("widgets");
        assert_eq!(
            widgets.on_key(&right),
            KeyResponse::Handled(Some(Msg::Toggle(3, true)))
        );
        let file = find_widget(&t, row("main.rs")).expect("main.rs");
        assert_eq!(file.on_key(&right), KeyResponse::Ignored);
        assert_eq!(file.on_click(), Some(Msg::Select(2)));
        // Down from src lands on main.rs, the next row.
        let next = ui
            .focus_directional(row("src"), crate::FocusDirection::Down)
            .expect("a row below");
        assert_eq!(next, row("main.rs"));
    }

    #[test]
    fn the_icon_slot_and_the_theme_are_used() {
        let mut theme = Theme::light();
        theme.widgets.file_tree.row_height = Some(40.0);
        let t = FileTree::<Msg>::new(sample()).icon(|node, _| Text::new(format!("[{}]", node.id)));
        let ui = build_ui(&t, Size::new(300.0, 400.0), &Runtime::default(), &theme);
        let words: Vec<String> = ui
            .scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                frus_core::Primitive::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(words.iter().any(|w| w == "[1]"), "{words:?}");
        let src = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some("src"))
            .map(|(_, r, _)| *r)
            .expect("src");
        assert_eq!(src.height, 40.0);
    }
}
