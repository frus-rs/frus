// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A large file tree through the shell (milestone 855): its rows built as they scroll into
//! view, the keys moving the focus between rows that are not built yet, and clicks with Shift
//! or the platform's own modifier building a selection — the keys and the clicks as the
//! keyboard and the mouse send them.

use std::collections::HashSet;

use crate::app::testing::Driver;
use crate::{Application, Command};
use frus_widgets::{
    Container, FileNode, FileTree, KeyStroke, Point, Role, ShortcutKey, Theme, Widget,
};

const W: f32 = 400.0;
const H: f32 = 300.0;
/// Folders of files under the root, each holding `FILES`.
const FOLDERS: u64 = 20;
const FILES: u64 = 500;

#[derive(Clone, Debug)]
enum Msg {
    Toggle(u64, bool),
    Selected(Vec<u64>),
}

/// A project of 20 folders of 500 files each, every folder open: 10,020 rows.
struct Explorer {
    open: HashSet<u64>,
    selection: Vec<u64>,
}

fn folder_id(f: u64) -> u64 {
    f * 1_000
}

fn file_id(f: u64, i: u64) -> u64 {
    f * 1_000 + i + 1
}

impl Application for Explorer {
    type Message = Msg;

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Toggle(id, true) => {
                self.open.insert(id);
            }
            Msg::Toggle(id, false) => {
                self.open.remove(&id);
            }
            Msg::Selected(ids) => self.selection = ids,
        }
        Command::none()
    }

    fn view(&self, _theme: &Theme) -> Box<dyn Widget<Msg>> {
        let nodes = (0..FOLDERS).map(|f| {
            FileNode::folder(
                folder_id(f),
                format!("folder {f:02}"),
                (0..FILES)
                    .map(move |i| FileNode::file(file_id(f, i), format!("f{f:02} file {i:03}"))),
            )
        });
        Box::new(
            Container::new().width(W).height(H).child(
                FileTree::new(nodes)
                    .expanded(self.open.iter().copied())
                    .multi_select(true)
                    .selection(self.selection.iter().copied())
                    .on_toggle(Msg::Toggle)
                    .on_selection_change(Msg::Selected)
                    .label("Project"),
            ),
        )
    }
}

fn open() -> Driver<Explorer> {
    let mut d = Driver::new(
        Explorer {
            open: (0..FOLDERS).map(folder_id).collect(),
            selection: Vec::new(),
        },
        W,
        H,
    );
    d.run(0.2);
    d
}

/// The rows the frame shows, by name, top to bottom.
fn shown(d: &Driver<Explorer>) -> Vec<String> {
    d.texts()
        .into_iter()
        .filter(|(t, r)| (t.starts_with("folder") || t.contains("file")) && r.y < H)
        .map(|(t, _)| t)
        .collect()
}

/// The name of the row with the focus, read from what the frame tells a screen reader.
fn focused(d: &Driver<Explorer>) -> Option<String> {
    let id = d.focused()?;
    let (ui, _) = d.frame_parts()?;
    ui.semantics()
        .iter()
        .find(|(node, _, s)| *node == id && s.role == Role::TreeItem)
        .and_then(|(_, _, s)| s.label.clone())
}

fn press(d: &mut Driver<Explorer>, key: KeyStroke) {
    d.key(key);
    d.run(0.6);
}

fn click(d: &mut Driver<Explorer>, name: &str) {
    let (_, at) = d
        .texts()
        .into_iter()
        .find(|(t, _)| t == name)
        .unwrap_or_else(|| panic!("{name} on screen: {:?}", shown(d)));
    let point = Point::new(at.x + 4.0, at.y + at.height * 0.5);
    d.press_mouse(point);
    d.run(0.02);
    d.release_mouse(point);
    d.run(0.3);
}

/// A click focuses a row, and the keys go on from it: Down to the next row, End to the last
/// of ten thousand — not built until the tree scrolled to it — and Home back to the top;
/// Left goes up to the folder, Left again closes it.
#[test]
fn the_keys_move_the_focus_through_ten_thousand_rows() {
    let mut d = open();
    let rows = shown(&d);
    assert!(
        rows.len() < 20,
        "only the window's rows are built: {rows:?}"
    );
    click(&mut d, "f00 file 001");
    assert_eq!(focused(&d).as_deref(), Some("f00 file 001"));
    assert_eq!(d.app().selection, [file_id(0, 1)]);

    press(&mut d, KeyStroke::new(ShortcutKey::Down));
    assert_eq!(focused(&d).as_deref(), Some("f00 file 002"));

    press(&mut d, KeyStroke::new(ShortcutKey::End));
    assert_eq!(focused(&d).as_deref(), Some("f19 file 499"));
    assert!(
        shown(&d).iter().any(|t| t == "f19 file 499"),
        "the last row scrolled into view: {:?}",
        shown(&d)
    );

    press(&mut d, KeyStroke::new(ShortcutKey::Left));
    assert_eq!(focused(&d).as_deref(), Some("folder 19"));
    press(&mut d, KeyStroke::new(ShortcutKey::Left));
    assert!(!d.app().open.contains(&folder_id(19)), "Left closed it");

    press(&mut d, KeyStroke::new(ShortcutKey::Home));
    assert_eq!(focused(&d).as_deref(), Some("folder 00"));
    assert!(shown(&d).iter().any(|t| t == "folder 00"));

    // A letter goes to the next row whose name starts with it.
    press(&mut d, KeyStroke::new(ShortcutKey::Char('f')));
    assert_eq!(focused(&d).as_deref(), Some("f00 file 000"));
}

/// Shift and the platform's own modifier with a click, as an explorer has them: Ctrl adds a
/// row to the selection, Shift selects the range from the row clicked last.
#[test]
fn clicks_with_modifiers_select_several_rows() {
    let mut d = open();
    click(&mut d, "f00 file 001");
    d.hold_modifiers(true, false, false, false);
    click(&mut d, "f00 file 003");
    assert_eq!(d.app().selection, [file_id(0, 1), file_id(0, 3)]);
    d.hold_modifiers(false, true, false, false);
    click(&mut d, "f00 file 005");
    assert_eq!(
        d.app().selection,
        (3..=5).map(|i| file_id(0, i)).collect::<Vec<_>>(),
        "the range from the row Ctrl-clicked last"
    );
    d.hold_modifiers(false, false, false, false);
    // Shift with the arrows extends it.
    press(&mut d, KeyStroke::new(ShortcutKey::Down).shift());
    assert_eq!(
        d.app().selection,
        (3..=6).map(|i| file_id(0, i)).collect::<Vec<_>>()
    );
}
