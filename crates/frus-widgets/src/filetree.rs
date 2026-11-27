// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`FileTree`]: folders and files, opened and closed in place — a project's explorer, a
//! document outline, a settings hierarchy (milestones 811 and 855).
//!
//! The application holds the nodes, which ones are open and which are selected; the tree
//! shows the open part of the hierarchy, one row per visible node, and says what was asked
//! of it. A folder whose children are not known yet is a **lazy** folder: opening it asks
//! the application to load them, and a loading row stands under it until they come.
//!
//! The open part is flattened into a list and shown by a virtualised [`ListView`]: only the
//! rows in view are built, so a tree of fifty thousand visible rows costs what its window
//! shows (milestone 855).
//!
//! The keyboard works as in a desktop explorer: Up and Down go from row to row, Right opens
//! a closed folder and goes into an open one, Left closes an open folder and goes up to the
//! parent of anything else, Home and End go to the first and the last row, Enter and Space
//! act on the row, and letters typed go to the next row whose name starts with them. With
//! [multiple selection](FileTree::multi_select), the platform's own modifier (Cmd on an
//! Apple keyboard, Ctrl elsewhere) adds a row to the selection or takes it out, and Shift
//! extends a range. A screen reader hears a tree of items, each with its name, its level,
//! its place among its siblings, whether it is selected and, for a folder, whether it is
//! open.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use frus_core::{Color, Rect, Role, Scene, SemanticsProperties, TextDirection, TextStyle};
use frus_layout::{Align, Style};

use crate::flex::Flex;
use crate::icons::Icons;
use crate::interaction::Status;
use crate::list::ListView;
use crate::rowkeys::{
    ClickModifiers, ModifiedClick, RowKey, RowKeyAnswer, RowKeyHandler, RowTarget,
};
use crate::shortcuts::ShortcutKey;
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::{sizing_of, Widget};

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

/// How long a pause ends a type-ahead: the letters typed within it make one name to find,
/// a letter after it starts a new one (milestone 855).
pub const FILE_TREE_TYPE_AHEAD_PAUSE: f32 = 1.0;

/// What a line of the open part of the tree is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineKind {
    /// A node: its id, whether it is a folder and open, and its place among its siblings,
    /// `(position, count)` counted from 1 (milestone 848).
    Node {
        id: u64,
        folder: bool,
        open: bool,
        place: (u32, u32),
    },
    /// The row under an open folder whose children have not come yet.
    Loading,
}

/// One line of the open part of the tree, as the list shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    /// The line of the folder it is in; `None` at the root.
    parent: Option<u32>,
    /// Which of its folder's children (or of the roots) it is.
    child: u32,
    /// How deep it is, `0` at the root.
    depth: u32,
    kind: LineKind,
}

impl Line {
    fn id(&self) -> Option<u64> {
        match self.kind {
            LineKind::Node { id, .. } => Some(id),
            LineKind::Loading => None,
        }
    }
}

/// The lines the open part of `nodes` makes, in reading order (milestone 855): one walk, each
/// line knowing its folder's line, so that Left can go up to it, and its place among its
/// siblings.
fn flatten(roots: &[FileNode], expanded: &HashSet<u64>) -> Vec<Line> {
    fn walk(
        nodes: &[FileNode],
        parent: Option<u32>,
        depth: u32,
        expanded: &HashSet<u64>,
        out: &mut Vec<Line>,
    ) {
        let count = nodes.len() as u32;
        for (i, node) in nodes.iter().enumerate() {
            let folder = node.is_folder();
            let open = folder && expanded.contains(&node.id);
            let me = out.len() as u32;
            out.push(Line {
                parent,
                child: i as u32,
                depth,
                kind: LineKind::Node {
                    id: node.id,
                    folder,
                    open,
                    place: (i as u32 + 1, count),
                },
            });
            if open {
                match &node.kind {
                    FileNodeKind::Folder(Some(children)) => {
                        walk(children, Some(me), depth + 1, expanded, out)
                    }
                    FileNodeKind::Folder(None) => out.push(Line {
                        parent: Some(me),
                        child: 0,
                        depth: depth + 1,
                        kind: LineKind::Loading,
                    }),
                    FileNodeKind::File => {}
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(roots, None, 0, expanded, &mut out);
    out
}

/// The node line `at` stands for: down from the roots along its folders, `O(depth)`.
fn node_at<'a>(roots: &'a [FileNode], lines: &[Line], at: usize) -> &'a FileNode {
    let line = lines[at];
    let siblings: &'a [FileNode] = match line.parent {
        None => roots,
        Some(parent) => match &node_at(roots, lines, parent as usize).kind {
            FileNodeKind::Folder(Some(children)) => children,
            _ => unreachable!("a line under a folder is under one with children"),
        },
    };
    &siblings[line.child as usize]
}

/// What a file tree keeps from one key to the next (milestone 855): where a range selection
/// starts, and the letters typed to find a row, with when the last one came.
#[derive(Debug, Default)]
pub(crate) struct TreeKeys {
    anchor: Cell<Option<u64>>,
    typed: RefCell<String>,
    last: Cell<f32>,
}

/// The settings a row is drawn with, resolved once per build.
#[derive(Clone, Copy)]
struct Look {
    row_height: f32,
    indent: f32,
    icon_size: f32,
    icon_color: Color,
    folder_color: Color,
    text_style: TextStyle,
    text_color: Color,
    selected_color: Color,
    selected_text_color: Color,
    loading_color: Color,
    rtl: bool,
}

type Toggle<Msg> = Rc<dyn Fn(u64, bool) -> Msg>;
type OnId<Msg> = Rc<dyn Fn(u64) -> Msg>;
type OnIds<Msg> = Rc<dyn Fn(Vec<u64>) -> Msg>;

/// Everything the rows of one build share: the nodes, their lines, the selection and what to
/// say about it.
struct Model<Msg> {
    roots: Rc<Vec<FileNode>>,
    lines: Vec<Line>,
    selection: HashSet<u64>,
    multi: bool,
    follow: bool,
    on_toggle: Option<Toggle<Msg>>,
    on_select: Option<OnId<Msg>>,
    on_selection: Option<OnIds<Msg>>,
    on_activate: Option<OnId<Msg>>,
    icon: Option<IconBuilder<Msg>>,
    loading_label: String,
    keys: Rc<TreeKeys>,
    pause: f32,
    look: Look,
}

impl<Msg: Clone + 'static> Model<Msg> {
    fn node(&self, at: usize) -> &FileNode {
        node_at(&self.roots, &self.lines, at)
    }

    fn target(&self, at: usize) -> Option<RowTarget> {
        self.lines[at].id().map(|key| RowTarget { index: at, key })
    }

    /// Whether a click or a key can select anything.
    fn selectable(&self) -> bool {
        self.on_select.is_some() || self.on_selection.is_some()
    }

    /// The node lines from `from` on (or back, `step` −1), skipping loading rows.
    fn next_node(&self, from: usize, forward: bool) -> Option<usize> {
        if forward {
            (from + 1..self.lines.len()).find(|&i| self.lines[i].id().is_some())
        } else {
            (0..from).rev().find(|&i| self.lines[i].id().is_some())
        }
    }

    /// The message that makes `ids` the selection, `lead` the row acted on: the selection's
    /// callback in a tree that selects several, `on_select(lead)` otherwise — each standing
    /// in for the other when it is the only one given.
    fn select(&self, ids: Vec<u64>, lead: u64) -> Option<Msg> {
        match (&self.on_selection, &self.on_select) {
            (Some(all), _) if self.multi => Some(all(ids)),
            (_, Some(one)) => Some(one(lead)),
            (Some(all), None) => Some(all(vec![lead])),
            (None, None) => None,
        }
    }

    /// Only `id`.
    fn select_only(&self, id: u64) -> Option<Msg> {
        self.select(vec![id], id)
    }

    /// `id` added to the selection, or taken out of it.
    fn toggle_selected(&self, id: u64) -> Option<Msg> {
        if !self.multi {
            return self.select_only(id);
        }
        let mut ids = self.selection_in_order();
        match ids.iter().position(|&s| s == id) {
            Some(at) => {
                ids.remove(at);
            }
            None => {
                ids.push(id);
                ids = self.ordered(ids);
            }
        }
        self.select(ids, id)
    }

    /// The rows from the anchor to line `to` selected, and only those. The anchor is the row
    /// last chosen without Shift; with none in view, the first selected row shown, and with
    /// none of those, line `from` — which then becomes the anchor.
    fn select_range(&self, from: usize, to: usize) -> Option<Msg> {
        let lead = self.lines[to].id()?;
        if !self.multi {
            return self.select_only(lead);
        }
        let line_of = |id: u64| self.lines.iter().position(|l| l.id() == Some(id));
        let anchor = self
            .keys
            .anchor
            .get()
            .and_then(line_of)
            .or_else(|| {
                self.lines
                    .iter()
                    .position(|l| l.id().is_some_and(|id| self.selection.contains(&id)))
            })
            .unwrap_or(from);
        self.keys.anchor.set(self.lines[anchor].id());
        let (lo, hi) = (anchor.min(to), anchor.max(to));
        let ids = self.lines[lo..=hi].iter().filter_map(Line::id).collect();
        self.select(ids, lead)
    }

    /// The selection as the rows show it, top to bottom, and then whatever is selected
    /// inside a closed folder, by id.
    fn selection_in_order(&self) -> Vec<u64> {
        self.ordered(self.selection.iter().copied().collect())
    }

    fn ordered(&self, ids: Vec<u64>) -> Vec<u64> {
        let wanted: HashSet<u64> = ids.iter().copied().collect();
        let mut shown: Vec<u64> = self
            .lines
            .iter()
            .filter_map(Line::id)
            .filter(|id| wanted.contains(id))
            .collect();
        let seen: HashSet<u64> = shown.iter().copied().collect();
        let mut hidden: Vec<u64> = ids.into_iter().filter(|id| !seen.contains(id)).collect();
        hidden.sort_unstable();
        hidden.dedup();
        shown.extend(hidden);
        shown
    }

    /// What a click on line `at` sends, the modifiers held: the platform's own modifier adds
    /// the row to the selection or takes it out, Shift selects the range from the anchor,
    /// nothing selects the row alone. A tree where nothing is selectable opens and closes a
    /// folder instead.
    fn click(&self, at: usize, modifiers: ClickModifiers) -> Option<Msg> {
        // A row chosen without Shift is where the next range starts.
        if !modifiers.shift {
            self.keys.anchor.set(self.lines[at].id());
        }
        self.click_message(at, modifiers)
    }

    /// The message of [`click`](Self::click), and nothing else.
    fn click_message(&self, at: usize, modifiers: ClickModifiers) -> Option<Msg> {
        let LineKind::Node {
            id, folder, open, ..
        } = self.lines[at].kind
        else {
            return None;
        };
        if !self.selectable() {
            return if folder {
                self.on_toggle.as_ref().map(|f| f(id, !open))
            } else {
                None
            };
        }
        if modifiers.shift {
            self.select_range(at, at)
        } else if modifiers.primary {
            self.toggle_selected(id)
        } else {
            self.select_only(id)
        }
    }

    /// Enter: the row's activation when the tree has one, otherwise what a click does.
    fn activate(&self, at: usize) -> Option<Msg> {
        let id = self.lines[at].id()?;
        match &self.on_activate {
            Some(activate) => Some(activate(id)),
            None => self.click(at, ClickModifiers::default()),
        }
    }

    /// The answer when the focus moves from line `from` to line `to`: with Shift, the range
    /// to it is selected; with the selection following the focus, it alone — unless the
    /// platform's own modifier is held, which moves the focus and nothing else.
    fn move_to(
        &self,
        from: usize,
        to: Option<usize>,
        extend: bool,
        keep: bool,
    ) -> RowKeyAnswer<Msg> {
        let Some(to) = to else {
            // At an end: the key is the tree's still, and nothing moves.
            return RowKeyAnswer::taken();
        };
        let Some(target) = self.target(to) else {
            return RowKeyAnswer::taken();
        };
        let message = if extend && self.selectable() {
            self.select_range(from, to)
        } else if self.follow && !keep && self.selectable() {
            self.keys.anchor.set(Some(target.key));
            self.select_only(target.key)
        } else {
            None
        };
        RowKeyAnswer::taken().send(message).focus(target)
    }

    /// The next row whose name starts with the letters typed, from line `from` on, wrapping
    /// round: a letter typed again and again goes from one row starting with it to the next.
    fn type_ahead(&self, from: usize, typed: char, at: f32) -> Option<usize> {
        let mut letters = self.keys.typed.borrow_mut();
        if at - self.keys.last.get() > self.pause {
            letters.clear();
        }
        self.keys.last.set(at);
        letters.extend(typed.to_lowercase());
        let first = letters.chars().next()?;
        let repeated = letters.chars().all(|c| c == first);
        // One letter, or the same one again: the next row with it. More: the row in focus
        // still counts, as the name typed so far may be its own.
        let (needle, start) = if repeated {
            (first.to_string(), from + 1)
        } else {
            (letters.clone(), from)
        };
        let n = self.lines.len();
        (0..n).map(|k| (start + k) % n).find(|&i| {
            self.lines[i].id().is_some() && self.node(i).label.to_lowercase().starts_with(&needle)
        })
    }

    /// What the row on line `at` does with `key` (milestone 855).
    fn key(&self, at: usize, key: RowKey) -> Option<RowKeyAnswer<Msg>> {
        let LineKind::Node {
            id, folder, open, ..
        } = self.lines[at].kind
        else {
            return None;
        };
        let stroke = key.stroke;
        let primary = key.primary();
        if stroke.alt {
            return None;
        }
        // Right goes in, Left goes out — the other way round in a right-to-left tree.
        let (inward, outward) = if self.look.rtl {
            (ShortcutKey::Left, ShortcutKey::Right)
        } else {
            (ShortcutKey::Right, ShortcutKey::Left)
        };
        let k = stroke.key;
        if k == ShortcutKey::Down || k == ShortcutKey::Up {
            let to = self.next_node(at, k == ShortcutKey::Down);
            return Some(self.move_to(at, to, stroke.shift, primary));
        }
        if k == ShortcutKey::Home || k == ShortcutKey::End {
            let to = if k == ShortcutKey::Home {
                (0..self.lines.len()).find(|&i| self.lines[i].id().is_some())
            } else {
                (0..self.lines.len())
                    .rev()
                    .find(|&i| self.lines[i].id().is_some())
            };
            return Some(self.move_to(at, to, stroke.shift, primary));
        }
        if k == inward {
            if stroke.shift || primary {
                return None;
            }
            if folder && !open {
                let message = self.on_toggle.as_ref().map(|f| f(id, true));
                return Some(RowKeyAnswer::taken().send(message));
            }
            // Open: into its first child, unless what is under it is still loading.
            let child = Some(at + 1).filter(|&i| {
                i < self.lines.len()
                    && self.lines[i].parent == Some(at as u32)
                    && self.lines[i].id().is_some()
            });
            return Some(match child {
                Some(child) if folder => self.move_to(at, Some(child), false, false),
                _ => RowKeyAnswer::taken(),
            });
        }
        if k == outward {
            if stroke.shift || primary {
                return None;
            }
            if folder && open {
                let message = self.on_toggle.as_ref().map(|f| f(id, false));
                return Some(RowKeyAnswer::taken().send(message));
            }
            let parent = self.lines[at].parent.map(|p| p as usize);
            return Some(match parent {
                Some(parent) => self.move_to(at, Some(parent), false, false),
                None => RowKeyAnswer::taken(),
            });
        }
        match k {
            ShortcutKey::Enter if !primary && !stroke.shift => {
                Some(RowKeyAnswer::taken().send(self.activate(at)))
            }
            ShortcutKey::Space => {
                let message = if !self.selectable() {
                    self.click(at, ClickModifiers::default())
                } else if stroke.shift {
                    self.select_range(at, at)
                } else {
                    self.keys.anchor.set(Some(id));
                    if self.multi {
                        self.toggle_selected(id)
                    } else {
                        self.select_only(id)
                    }
                };
                Some(RowKeyAnswer::taken().send(message))
            }
            ShortcutKey::Char(c) if primary && self.multi && c.eq_ignore_ascii_case(&'a') => {
                let all: Vec<u64> = self.lines.iter().filter_map(Line::id).collect();
                Some(RowKeyAnswer::taken().send(self.select(all, id)))
            }
            ShortcutKey::Char(c) if !primary && !stroke.ctrl && !stroke.meta => {
                if c.is_whitespace() || c.is_control() {
                    return None;
                }
                let to = self.type_ahead(at, c, key.at);
                Some(match to {
                    Some(to) if to != at => self.move_to(at, Some(to), false, false),
                    _ => RowKeyAnswer::taken(),
                })
            }
            _ => None,
        }
    }

    /// The row on line `at`.
    fn row(self: &Rc<Self>, at: usize) -> Box<dyn Widget<Msg>> {
        let look = self.look;
        let line = self.lines[at];
        let LineKind::Node {
            id,
            folder,
            open,
            place,
        } = line.kind
        else {
            // The words under a folder still loading: not a row of the tree.
            return Box::new(
                Flex::row()
                    .align(Align::Center)
                    .height(look.row_height)
                    .padding_each(
                        0.0,
                        0.0,
                        0.0,
                        line.depth as f32 * look.indent + look.icon_size + 6.0,
                    )
                    .child(
                        Text::styled(self.loading_label.clone(), look.text_style)
                            .italic()
                            .color(look.loading_color),
                    ),
            );
        };
        let node = self.node(at);
        let selected = self.selection.contains(&id);
        let fg = if selected {
            look.selected_text_color
        } else {
            look.text_color
        };
        let mut content = Flex::row()
            .align(Align::Center)
            .height(look.row_height)
            .child(crate::SizedBox::<Msg>::empty().width(line.depth as f32 * look.indent));
        // The chevron: its own target, so a click on it opens or closes, and a click on the
        // rest of the row selects.
        let chevron: Box<dyn Widget<Msg>> = if folder {
            let glyph = crate::Icon::new(if open {
                Icons::EXPAND_MORE
            } else if look.rtl {
                Icons::CHEVRON_LEFT
            } else {
                Icons::CHEVRON_RIGHT
            })
            .size(look.icon_size)
            .color(look.icon_color);
            let boxed = crate::Container::new()
                .width(look.icon_size + 6.0)
                .height(look.row_height)
                .alignment(frus_core::Alignment::CENTER)
                .child(glyph);
            match self.on_toggle.as_ref().map(|f| f(id, !open)) {
                Some(message) => Box::new(crate::GestureDetector::new(boxed).on_tap(message)),
                None => Box::new(boxed),
            }
        } else {
            Box::new(crate::SizedBox::<Msg>::empty().width(look.icon_size + 6.0))
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
                .size(look.icon_size)
                .color(if folder {
                    look.folder_color
                } else {
                    look.icon_color
                }),
            ),
        };
        content = content
            .child(icon)
            .child(crate::SizedBox::<Msg>::empty().width(6.0))
            .child(crate::Expanded::new(
                Text::styled(node.label.clone(), look.text_style)
                    .color(fg)
                    .no_wrap()
                    .ellipsis(),
            ));
        Box::new(FileRow {
            id,
            level: line.depth + 1,
            place: (place.0 as usize, place.1 as usize),
            label: node.label.clone(),
            folder,
            open,
            selected,
            click: self.click_message(at, ClickModifiers::default()),
            model: Rc::clone(self),
            at,
            height: look.row_height,
            selected_color: look.selected_color,
            children: vec![Box::new(crate::ExcludeSemantics::new(content))],
        })
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
///
/// The tree **scrolls**, and builds only the rows in view (milestone 855): it takes the room
/// on offer, as a [`ListView`] does, or the height it is given ([`FileTree::height`],
/// [`FileTree::flex`]); [`FileTree::shrink_wrap`] makes it as tall as its rows instead, for
/// a short tree in a page that scrolls.
///
/// It selects one row, or several with [`FileTree::multi_select`]:
///
/// ```
/// use frus_widgets::{FileNode, FileTree};
///
/// #[derive(Clone)]
/// enum Msg { Selected(Vec<u64>), Open(u64) }
///
/// let _tree: FileTree<Msg> = FileTree::new(vec![FileNode::file(1, "a.rs"), FileNode::file(2, "b.rs")])
///     .multi_select(true)
///     .selection([1])
///     .on_selection_change(Msg::Selected)
///     .on_activate(Msg::Open)
///     .height(320.0);
/// ```
pub struct FileTree<Msg = crate::callback::Callback> {
    roots: Rc<Vec<FileNode>>,
    expanded: HashSet<u64>,
    selection: HashSet<u64>,
    multi: bool,
    follow: bool,
    on_toggle: Option<Toggle<Msg>>,
    on_select: Option<OnId<Msg>>,
    on_selection: Option<OnIds<Msg>>,
    on_activate: Option<OnId<Msg>>,
    icon: Option<IconBuilder<Msg>>,
    label: Option<String>,
    loading_label: String,
    pause: f32,
    height: Option<f32>,
    flex: f32,
    shrink_wrap: bool,
    /// What the tree keeps between keys, found in the runtime by its identity.
    keys: RefCell<Option<Rc<TreeKeys>>>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> FileTree<Msg> {
    /// A tree of `roots`, every folder closed.
    pub fn new(roots: impl IntoIterator<Item = FileNode>) -> Self {
        Self {
            roots: Rc::new(roots.into_iter().collect()),
            expanded: HashSet::new(),
            selection: HashSet::new(),
            multi: false,
            follow: false,
            on_toggle: None,
            on_select: None,
            on_selection: None,
            on_activate: None,
            icon: None,
            label: None,
            loading_label: crate::localizations::of().loading_label().into(),
            pause: FILE_TREE_TYPE_AHEAD_PAUSE,
            height: None,
            flex: 0.0,
            shrink_wrap: false,
            keys: RefCell::new(None),
            built: OnceCell::new(),
        }
    }

    /// The folders that are open.
    pub fn expanded(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.expanded = ids.into_iter().collect();
        self
    }

    /// The selected node, highlighted — the whole selection, in a tree that selects one.
    pub fn selected(mut self, id: Option<u64>) -> Self {
        self.selection = id.into_iter().collect();
        self
    }

    /// The selected nodes, highlighted (milestone 855); see [`FileTree::multi_select`].
    pub fn selection(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.selection = ids.into_iter().collect();
        self
    }

    /// **Selects several rows** (milestone 855): a click with the platform's own modifier
    /// (Cmd on an Apple keyboard, Ctrl elsewhere) or Space adds a row to the selection or
    /// takes it out, Shift with a click, an arrow, Home, End or Space selects the range from
    /// the row last chosen, and the platform's modifier with A selects every row shown. The
    /// new selection is sent to [`FileTree::on_selection_change`]. Off by default: one row.
    pub fn multi_select(mut self, multi: bool) -> Self {
        self.multi = multi;
        self
    }

    /// Whether moving the focus with the keys **selects** the row it lands on (milestone
    /// 855), as an explorer's list does. Off by default: the arrows move the focus and Space
    /// or Enter selects, so that a selection that opens a document does not open every one
    /// passed on the way.
    pub fn selection_follows_focus(mut self, follow: bool) -> Self {
        self.follow = follow;
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

    /// `on_select(id)`: a node is to be selected. In a tree that
    /// [selects several](FileTree::multi_select), [`FileTree::on_selection_change`] is sent
    /// instead when there is one.
    pub fn on_select<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_select: impl Fn(u64) -> R + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(crate::callback::handler1(on_select)));
        self
    }

    /// `on_selection_change(ids)`: the selection is to become `ids`, top to bottom as the
    /// rows show them (milestone 855). In a tree that selects one, `ids` is that one, when
    /// there is no [`FileTree::on_select`].
    pub fn on_selection_change<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_selection: impl Fn(Vec<u64>) -> R + 'static,
    ) -> Self {
        self.on_selection = Some(Rc::new(crate::callback::handler1(on_selection)));
        self
    }

    /// `on_activate(id)`: Enter on a row — open the file (milestone 855). Without it, Enter
    /// does what a click does.
    pub fn on_activate<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_activate: impl Fn(u64) -> R + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(crate::callback::handler1(on_activate)));
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

    /// How long a pause ends a type-ahead, in seconds (milestone 855); see
    /// [`FILE_TREE_TYPE_AHEAD_PAUSE`].
    pub fn type_ahead_pause(mut self, seconds: f32) -> Self {
        self.pause = seconds.max(0.0);
        self
    }

    /// The tree's height, in logical pixels: its window onto the rows (milestone 855).
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Flex growth along the parent's main axis: the tree takes that share of the room left.
    pub fn flex(mut self, grow: f32) -> Self {
        self.flex = grow;
        self
    }

    /// **As tall as its rows** (milestone 855): no scrolling of its own, every row built —
    /// a short tree in a page that scrolls. Off by default.
    pub fn shrink_wrap(mut self, shrink: bool) -> Self {
        self.shrink_wrap = shrink;
        self
    }

    /// The list of rows, built under `theme`.
    fn build(&self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let t = theme.widgets.file_tree;
        let look = Look {
            row_height: t.row_height.unwrap_or(FILE_TREE_ROW_HEIGHT),
            indent: t.indent.unwrap_or(FILE_TREE_INDENT),
            icon_size: t.icon_size.unwrap_or(18.0),
            icon_color: t.icon_color.unwrap_or(theme.scheme.on_surface_variant),
            folder_color: t.folder_color.unwrap_or(theme.scheme.primary),
            text_style: t.text_style.unwrap_or(theme.text.body_medium),
            text_color: t.text_color.unwrap_or(theme.scheme.on_surface),
            selected_color: t.selected_color.unwrap_or(theme.scheme.secondary_container),
            selected_text_color: t
                .selected_text_color
                .unwrap_or(theme.scheme.on_secondary_container),
            loading_color: theme.scheme.on_surface_variant,
            rtl: theme.direction == TextDirection::Rtl,
        };
        let lines = flatten(&self.roots, &self.expanded);
        let count = lines.len();
        let keys = self.keys.borrow().clone().unwrap_or_default();
        let model = Rc::new(Model {
            roots: Rc::clone(&self.roots),
            lines,
            selection: self.selection.clone(),
            multi: self.multi,
            follow: self.follow,
            on_toggle: self.on_toggle.clone(),
            on_select: self.on_select.clone(),
            on_selection: self.on_selection.clone(),
            on_activate: self.on_activate.clone(),
            icon: self.icon.clone(),
            loading_label: self.loading_label.clone(),
            keys,
            pause: self.pause,
            look,
        });
        let rows = Rc::clone(&model);
        let keys = Rc::clone(&model);
        let mut list = ListView::new(count, look.row_height, move |i| rows.row(i))
            // Each row's key is its node's id, known without building it.
            .item_keys(move |i| keys.lines.get(i).and_then(Line::id))
            // The tree is the container a reader enters; its rows say their own place.
            .semantic_list(false);
        if self.shrink_wrap {
            list = list.height(count as f32 * look.row_height).primary(false);
        } else if let Some(height) = self.height {
            list = list.height(height);
        }
        if self.flex > 0.0 {
            list = list.flex(self.flex);
        }
        Box::new(list)
    }

    fn list(&self) -> &dyn Widget<Msg> {
        self.children()[0].as_ref()
    }
}

/// One node's row: a focus stop that selects, opens and closes, and paints its selection.
struct FileRow<Msg: Clone + 'static> {
    id: u64,
    /// Its depth, `1` at the root, and its place among its siblings (milestone 848).
    level: u32,
    place: (usize, usize),
    label: String,
    folder: bool,
    open: bool,
    selected: bool,
    /// What a plain click sends.
    click: Option<Msg>,
    /// The tree it is a row of, and which line (milestone 855).
    model: Rc<Model<Msg>>,
    at: usize,
    height: f32,
    selected_color: Color,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> Widget<Msg> for FileRow<Msg> {
    fn style(&self) -> Style {
        Style {
            height: frus_layout::Dimension::Length(self.height),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    /// The row keeps its identity — its focus, its hover — however the rows above it change;
    /// and the tree names it by this when it sends the focus to it (milestone 855).
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

    /// A click selects; with nothing selectable, a folder opens or closes.
    fn on_click(&self) -> Option<Msg> {
        self.click.clone()
    }

    /// With the platform's own modifier, the row joins the selection or leaves it; with
    /// Shift, the range to it is selected (milestone 855).
    fn on_modified_click(&self) -> Option<ModifiedClick<Msg>> {
        let model = Rc::clone(&self.model);
        let at = self.at;
        Some(Rc::new(move |modifiers| model.click(at, modifiers)))
    }

    fn focusable(&self) -> bool {
        true
    }

    /// The tree's keys: the arrows, Home and End move between rows, Right and Left open,
    /// close and go in and out, Enter and Space act, letters find a row (milestone 855).
    fn on_row_key(&self) -> Option<RowKeyHandler<Msg>> {
        let model = Rc::clone(&self.model);
        let at = self.at;
        Some(Rc::new(move |key| model.key(at, key)))
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        let mut sem = SemanticsProperties::new(Role::TreeItem)
            .label(self.label.clone())
            .toggled(self.selected)
            .clickable()
            .level(self.level)
            .position(self.place.0, self.place.1);
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
    /// The list's own sizing: the tree is its window onto the rows.
    fn style(&self) -> Style {
        sizing_of(self.list().style())
    }

    fn build_in(
        &self,
        id: crate::interaction::WidgetId,
        runtime: &crate::runtime::Runtime,
        theme: &Theme,
    ) {
        // What the tree keeps between keys lives as long as the tree does in the runtime.
        if self.built.get().is_none() {
            let keys = runtime
                .file_trees
                .borrow_mut()
                .entry(id)
                .or_default()
                .clone();
            *self.keys.borrow_mut() = Some(keys);
        }
        self.build_themed(theme)
    }

    fn build_themed(&self, theme: &Theme) {
        let _ = self.built.get_or_init(|| vec![self.build(theme)]);
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built
            .get_or_init(|| vec![self.build(&Theme::default())])
    }

    /// It takes the room on offer where the list would.
    fn fill_axes(&self, theme: &Theme) -> crate::widget::FillAxes {
        self.list().fill_axes(theme)
    }

    fn soft_extent(&self) -> crate::widget::FillAxes {
        self.list().soft_extent()
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        let mut sem = SemanticsProperties::new(Role::Tree).multiselectable(self.multi);
        if let Some(label) = &self.label {
            sem = sem.label(label.clone());
        }
        Some(sem)
    }

    fn debug_name(&self) -> &'static str {
        "FileTree"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interaction::WidgetId;
    use crate::shortcuts::KeyStroke;
    use crate::{build_ui, Point, Runtime, Size, TargetPlatform, Ui};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Toggle(u64, bool),
        Select(u64),
        Selection(Vec<u64>),
        Open(u64),
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

    fn ui_of(t: &FileTree<Msg>, runtime: &Runtime) -> Ui<Msg> {
        build_ui(t, Size::new(300.0, 400.0), runtime, &Theme::light())
    }

    fn rows(t: &FileTree<Msg>) -> Vec<(String, bool)> {
        let ui = ui_of(t, &Runtime::default());
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

    /// The identity of the row named `name`.
    fn row(ui: &Ui<Msg>, name: &str) -> WidgetId {
        ui.semantics()
            .iter()
            .find(|(_, _, s)| s.role == Role::TreeItem && s.label.as_deref() == Some(name))
            .map(|(id, _, _)| *id)
            .unwrap_or_else(|| panic!("{name} is a row"))
    }

    fn stroke(key: ShortcutKey) -> KeyStroke {
        KeyStroke::new(key)
    }

    /// `key` pressed on the row named `on`, on a keyboard that is not Apple's.
    fn press(
        ui: &Ui<Msg>,
        runtime: &Runtime,
        on: &str,
        key: KeyStroke,
    ) -> Option<crate::RowKeyOutcome<Msg>> {
        press_at(ui, runtime, on, key, 0.0)
    }

    fn press_at(
        ui: &Ui<Msg>,
        runtime: &Runtime,
        on: &str,
        key: KeyStroke,
        at: f32,
    ) -> Option<crate::RowKeyOutcome<Msg>> {
        let key = RowKey {
            stroke: key,
            keyboard: TargetPlatform::Windows,
            at,
        };
        ui.row_key(row(ui, on), key, runtime)
    }

    /// The name of the row the focus went to.
    fn went(ui: &Ui<Msg>, outcome: &crate::RowKeyOutcome<Msg>) -> Option<String> {
        let to = outcome.focus?;
        ui.semantics()
            .iter()
            .find(|(id, _, _)| *id == to)
            .and_then(|(_, _, s)| s.label.clone())
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
    fn the_lines_know_their_folder_and_their_place() {
        let nodes = sample();
        let lines = flatten(&nodes, &[1, 3, 5].into_iter().collect());
        let ids: Vec<Option<u64>> = lines.iter().map(Line::id).collect();
        assert_eq!(
            ids,
            [Some(1), Some(2), Some(3), Some(4), Some(5), None, Some(6)]
        );
        // tree.rs is under widgets, which is under src.
        assert_eq!(lines[3].parent, Some(2));
        assert_eq!(lines[2].parent, Some(0));
        assert_eq!(lines[3].depth, 2);
        assert_eq!(node_at(&nodes, &lines, 3).label, "tree.rs");
        // The loading row is under assets, and is nobody's sibling.
        assert_eq!(lines[5].kind, LineKind::Loading);
        assert_eq!(lines[5].parent, Some(4));
        let place = |i: usize| match lines[i].kind {
            LineKind::Node { place, .. } => place,
            LineKind::Loading => panic!("not a node"),
        };
        assert_eq!(place(0), (1, 3));
        assert_eq!(place(6), (3, 3));
        assert_eq!(place(2), (2, 2));
    }

    #[test]
    fn a_lazy_folder_opened_shows_a_loading_row() {
        let ui = ui_of(&tree(&[5]), &Runtime::default());
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
        let ui = ui_of(&tree(&[1]).selected(Some(2)), &Runtime::default());
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
        assert_eq!(file.2.level, Some(2));
        assert_eq!(file.2.position, Some((1, 2)));
        assert_eq!(
            file.2.toggled,
            frus_core::Toggled::True,
            "main.rs is selected"
        );
        let src = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some("src"))
            .expect("src");
        assert_eq!(src.2.expanded, Some(true));
        assert_eq!(src.2.toggled, frus_core::Toggled::False);
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
        // The rows are the tree's items, with no list between them and it.
        assert!(
            !ui.semantics().iter().any(|(_, _, s)| s.role == Role::List),
            "no list role"
        );
    }

    /// A click on the chevron opens; on the rest of the row, selects.
    #[test]
    fn the_chevron_toggles_and_the_body_selects() {
        let ui = ui_of(&tree(&[]), &Runtime::default());
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
            ui.msg_for_click(id, ClickModifiers::default())
        };
        assert_eq!(click(src.x + 8.0), Some(Msg::Toggle(1, true)));
        assert_eq!(click(src.x + 150.0), Some(Msg::Select(1)));
    }

    /// Right opens a closed folder and goes into an open one; Left closes an open folder and
    /// goes up from anything else; Up and Down go from row to row, past a loading row.
    #[test]
    fn the_arrows_move_in_and_out_and_along() {
        let runtime = Runtime::default();
        let ui = ui_of(&tree(&[1, 5]), &runtime);
        let k = |on: &str, key: ShortcutKey| press(&ui, &runtime, on, stroke(key)).expect("taken");
        // Right on a closed folder opens it, and the focus stays.
        let open = k("widgets", ShortcutKey::Right);
        assert_eq!(open.messages, [Msg::Toggle(3, true)]);
        assert_eq!(open.focus, None);
        // Right on an open one goes to its first child.
        assert_eq!(
            went(&ui, &k("src", ShortcutKey::Right)).as_deref(),
            Some("main.rs")
        );
        // Right on a file does nothing, and keeps the key.
        let file = k("main.rs", ShortcutKey::Right);
        assert!(file.messages.is_empty() && file.focus.is_none());
        // Left on an open folder closes it; on a child it goes to the parent.
        assert_eq!(
            k("src", ShortcutKey::Left).messages,
            [Msg::Toggle(1, false)]
        );
        assert_eq!(
            went(&ui, &k("widgets", ShortcutKey::Left)).as_deref(),
            Some("src")
        );
        // At the root, a file's Left goes nowhere.
        let root = k("Cargo.toml", ShortcutKey::Left);
        assert!(root.messages.is_empty() && root.focus.is_none());
        // Up and Down, and the loading row under assets is passed over.
        assert_eq!(
            went(&ui, &k("src", ShortcutKey::Down)).as_deref(),
            Some("main.rs")
        );
        assert_eq!(
            went(&ui, &k("main.rs", ShortcutKey::Up)).as_deref(),
            Some("src")
        );
        assert_eq!(
            went(&ui, &k("assets", ShortcutKey::Down)).as_deref(),
            Some("Cargo.toml")
        );
        assert!(k("src", ShortcutKey::Up).focus.is_none(), "nothing above");
        // Home and End.
        assert_eq!(
            went(&ui, &k("widgets", ShortcutKey::Home)).as_deref(),
            Some("src")
        );
        assert_eq!(
            went(&ui, &k("main.rs", ShortcutKey::End)).as_deref(),
            Some("Cargo.toml")
        );
        // Enter and Space select; Tab is left to the focus.
        assert_eq!(k("main.rs", ShortcutKey::Enter).messages, [Msg::Select(2)]);
        assert_eq!(k("main.rs", ShortcutKey::Space).messages, [Msg::Select(2)]);
        assert!(press(&ui, &runtime, "main.rs", stroke(ShortcutKey::Tab)).is_none());
    }

    /// Right to left, the arrows swap: Left goes in, Right goes out.
    #[test]
    fn right_to_left_the_arrows_swap() {
        let runtime = Runtime::default();
        let theme = Theme {
            direction: TextDirection::Rtl,
            ..Theme::light()
        };
        let ui = build_ui(&tree(&[1]), Size::new(300.0, 400.0), &runtime, &theme);
        let k = |on: &str, key: ShortcutKey| press(&ui, &runtime, on, stroke(key)).expect("taken");
        assert_eq!(
            k("widgets", ShortcutKey::Left).messages,
            [Msg::Toggle(3, true)]
        );
        assert_eq!(
            k("src", ShortcutKey::Right).messages,
            [Msg::Toggle(1, false)]
        );
    }

    /// Enter activates when the tree says what that is.
    #[test]
    fn enter_activates() {
        let runtime = Runtime::default();
        let ui = ui_of(&tree(&[1]).on_activate(Msg::Open), &runtime);
        let out = press(&ui, &runtime, "main.rs", stroke(ShortcutKey::Enter)).expect("taken");
        assert_eq!(out.messages, [Msg::Open(2)]);
    }

    /// Letters typed go to the next row whose name starts with them; the same letter again
    /// goes on to the next; a pause starts a new name.
    #[test]
    fn letters_typed_find_a_row() {
        let runtime = Runtime::default();
        let t = FileTree::<Msg>::new(vec![
            FileNode::file(1, "alpha"),
            FileNode::file(2, "beta"),
            FileNode::file(3, "Bravo"),
            FileNode::file(4, "brief"),
            FileNode::file(5, "charlie"),
        ]);
        let ui = ui_of(&t, &runtime);
        let typed = |on: &str, c: char, at: f32| {
            let out = press_at(&ui, &runtime, on, stroke(ShortcutKey::Char(c)), at)
                .expect("a letter is taken");
            went(&ui, &out)
        };
        assert_eq!(typed("alpha", 'b', 0.0).as_deref(), Some("beta"));
        // The same letter again: the next one with it, in any case.
        assert_eq!(typed("beta", 'b', 0.2).as_deref(), Some("Bravo"));
        // Later, a new name: "b" goes on to brief, "br" stays there, "bra" goes back up
        // round the end to Bravo.
        assert_eq!(typed("Bravo", 'b', 5.0).as_deref(), Some("brief"));
        assert_eq!(typed("brief", 'r', 5.2), None, "brief is already it");
        assert_eq!(typed("brief", 'a', 5.4).as_deref(), Some("Bravo"));
        assert_eq!(typed("alpha", 'c', 9.0).as_deref(), Some("charlie"));
        // Round the end and back to the top.
        assert_eq!(typed("charlie", 'a', 12.0).as_deref(), Some("alpha"));
        // A letter with the platform's modifier is not typing.
        let ctrl_b = stroke(ShortcutKey::Char('b')).ctrl();
        assert!(press(&ui, &runtime, "alpha", ctrl_b).is_none());
    }

    fn multi(selection: &[u64]) -> FileTree<Msg> {
        FileTree::new((1..=6).map(|i| FileNode::file(i, format!("file {i}"))))
            .multi_select(true)
            .selection(selection.iter().copied())
            .on_selection_change(Msg::Selection)
    }

    fn click_on(ui: &Ui<Msg>, name: &str, modifiers: ClickModifiers) -> Option<Msg> {
        ui.msg_for_click(row(ui, name), modifiers)
    }

    /// The platform's modifier adds a row to the selection or takes it out; Shift selects
    /// the range from the row last chosen; a plain click selects the row alone.
    #[test]
    fn clicks_with_modifiers_build_a_selection() {
        let runtime = Runtime::default();
        let ui = ui_of(&multi(&[2]), &runtime);
        let toggle = ClickModifiers {
            primary: true,
            ..Default::default()
        };
        let extend = ClickModifiers {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            click_on(&ui, "file 4", toggle),
            Some(Msg::Selection(vec![2, 4]))
        );
        assert_eq!(
            click_on(&ui, "file 2", toggle),
            Some(Msg::Selection(vec![])),
            "toggled off"
        );
        // The anchor is the row last chosen without Shift: file 2.
        assert_eq!(
            click_on(&ui, "file 5", extend),
            Some(Msg::Selection(vec![2, 3, 4, 5]))
        );
        assert_eq!(
            click_on(&ui, "file 1", extend),
            Some(Msg::Selection(vec![1, 2])),
            "the anchor stays"
        );
        assert_eq!(
            click_on(&ui, "file 6", ClickModifiers::default()),
            Some(Msg::Selection(vec![6]))
        );
    }

    /// Shift with the arrows and Home or End extends the range; Space toggles; the platform's
    /// modifier with A selects every row.
    #[test]
    fn keys_build_a_selection() {
        let runtime = Runtime::default();
        let ui = ui_of(&multi(&[3]), &runtime);
        let k = |on: &str, key: KeyStroke| press(&ui, &runtime, on, key).expect("taken");
        // With no anchor yet, the range starts at the selected row.
        let down = k("file 3", stroke(ShortcutKey::Down).shift());
        assert_eq!(down.messages, [Msg::Selection(vec![3, 4])]);
        assert_eq!(went(&ui, &down).as_deref(), Some("file 4"));
        let end = k("file 4", stroke(ShortcutKey::End).shift());
        assert_eq!(end.messages, [Msg::Selection(vec![3, 4, 5, 6])]);
        let space = k("file 1", stroke(ShortcutKey::Space));
        assert_eq!(space.messages, [Msg::Selection(vec![1, 3])]);
        let all = k("file 1", stroke(ShortcutKey::Char('a')).ctrl());
        assert_eq!(all.messages, [Msg::Selection(vec![1, 2, 3, 4, 5, 6])]);
        let tree = ui.semantics().iter().find(|(_, _, s)| s.role == Role::Tree);
        assert!(
            tree.is_some_and(|(_, _, s)| s.multiselectable),
            "a reader is told"
        );
        // Without Shift, the arrows move the focus and nothing else.
        let plain = k("file 1", stroke(ShortcutKey::Down));
        assert!(plain.messages.is_empty());
        assert_eq!(went(&ui, &plain).as_deref(), Some("file 2"));
        // Cmd is the modifier on an Apple keyboard, and Ctrl is not.
        let apple = |key: KeyStroke| {
            ui.row_key(
                row(&ui, "file 1"),
                RowKey {
                    stroke: key,
                    keyboard: TargetPlatform::MacOs,
                    at: 0.0,
                },
                &runtime,
            )
        };
        assert!(apple(stroke(ShortcutKey::Char('a')).meta())
            .is_some_and(|o| o.messages == [Msg::Selection(vec![1, 2, 3, 4, 5, 6])]));
        assert!(apple(stroke(ShortcutKey::Char('a')).ctrl()).is_none());
    }

    /// With the selection following the focus, an arrow selects the row it lands on.
    #[test]
    fn the_selection_can_follow_the_focus() {
        let runtime = Runtime::default();
        let ui = ui_of(&tree(&[1]).selection_follows_focus(true), &runtime);
        let out = press(&ui, &runtime, "src", stroke(ShortcutKey::Down)).expect("taken");
        assert_eq!(out.messages, [Msg::Select(2)]);
        // The platform's modifier moves the focus alone.
        let out = press(&ui, &runtime, "src", stroke(ShortcutKey::Down).ctrl()).expect("taken");
        assert!(out.messages.is_empty());
    }

    /// **Only the rows in view are built** (milestone 855): fifty thousand rows under an open
    /// folder, a window of four hundred pixels, and the tree builds what the window shows.
    #[test]
    fn fifty_thousand_rows_build_only_the_window() {
        let built = Rc::new(Cell::new(0usize));
        let make = |built: &Rc<Cell<usize>>| {
            let count = Rc::clone(built);
            FileTree::<Msg>::new(vec![FileNode::folder(
                0,
                "big",
                (1..=50_000).map(|i| FileNode::file(i, format!("file {i}"))),
            )])
            .expanded([0])
            .icon(move |_, _| {
                count.set(count.get() + 1);
                crate::SizedBox::<Msg>::empty().width(18.0)
            })
        };
        let runtime = Runtime::default();
        let t = make(&built);
        let ui = ui_of(&t, &runtime);
        let first = built.get();
        // 400 / 28: fifteen rows, give or take one part shown.
        assert!(
            (14..=16).contains(&first),
            "{first} rows built for a window of 15"
        );
        // End, from the top: the focus goes to the last row, unbuilt, and the list scrolls
        // to it.
        let last = press(&ui, &runtime, "big", stroke(ShortcutKey::End)).expect("taken");
        let list = ui.scroll_regions()[0].id;
        assert_eq!(last.focus, Some(list.keyed(50_000)));
        let (area, offset) = last.scrolls[0];
        assert_eq!(area, list);
        let bottom = 50_001.0 * FILE_TREE_ROW_HEIGHT - 400.0;
        assert!((offset.1 - bottom).abs() < 0.5, "{offset:?} vs {bottom}");
        // At the bottom, the next frame builds the rows there, the last one the focus.
        let mut runtime = runtime;
        runtime.scroll.insert(list, offset);
        built.set(0);
        let t = make(&built);
        let ui = ui_of(&t, &runtime);
        assert!(
            built.get() <= 16,
            "{} rows built at the bottom",
            built.get()
        );
        assert_eq!(row(&ui, "file 50000"), list.keyed(50_000));
    }

    /// Unsized, the tree takes the room on offer; given a height, it keeps it; shrink-wrapped
    /// it is as tall as its rows.
    #[test]
    fn the_tree_is_a_window_onto_its_rows() {
        let height = |t: FileTree<Msg>| {
            let ui = ui_of(&t, &Runtime::default());
            ui.semantics()
                .iter()
                .find(|(_, _, s)| s.role == Role::Tree)
                .map(|(_, r, _)| r.height)
                .expect("a tree")
        };
        assert_eq!(height(tree(&[])), 400.0);
        assert_eq!(height(tree(&[]).height(120.0)), 120.0);
        assert_eq!(
            height(tree(&[1]).shrink_wrap(true)),
            5.0 * FILE_TREE_ROW_HEIGHT
        );
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
