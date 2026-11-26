// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Shortcuts written with the **primary modifier**, and the framework's editing shortcuts
//! overridden, driven through the shell (milestone 852, L-037): the keys as a keyboard sends
//! them, with their modifiers held, on an Apple keyboard and on another.

use crate::app::testing::Driver;
use crate::{Application, Command};
use frus_widgets::{
    Actions, Container, Flex, Intent, KeyStroke, Point, ShortcutKey, Shortcuts, TextField, Theme,
    Widget,
};

const W: f32 = 400.0;
const H: f32 = 200.0;

#[derive(Clone, Debug)]
enum Msg {
    Edited(String),
    Saved,
}

/// One field under an application's shortcuts: `primary+S` saves, `primary+U` undoes (the
/// framework's own Undo, bound to another key), and `primary+A` does nothing.
struct Form {
    value: String,
    saves: usize,
}

impl Application for Form {
    type Message = Msg;

    fn update(&mut self, msg: Msg) -> Command<Msg> {
        match msg {
            Msg::Edited(value) => self.value = value,
            Msg::Saved => self.saves += 1,
        }
        Command::none()
    }

    fn view(&self, _theme: &Theme) -> Box<dyn Widget<Msg>> {
        let field = TextField::new(self.value.clone())
            .on_input(Msg::Edited)
            .width(W - 40.0);
        let key = |c| KeyStroke::new(ShortcutKey::Char(c)).primary();
        let keys = Shortcuts::new(Flex::column().child(field))
            .bind(key('s'), Intent("save"))
            .bind(key('u'), Intent::UNDO)
            .bind(key('a'), Intent::DO_NOTHING);
        Box::new(
            Container::new()
                .width(W)
                .height(H)
                .child(Actions::new(keys).action(Intent("save"), Msg::Saved)),
        )
    }
}

/// The field holding `value`, focused, its caret at the end of the text.
fn open(value: &str, apple: bool) -> Driver<Form> {
    let mut d = Driver::new(
        Form {
            value: value.into(),
            saves: 0,
        },
        W,
        H,
    );
    d.set_apple_keyboard(apple);
    d.run(0.2);
    let (_, at) = d
        .texts()
        .into_iter()
        .find(|(text, _)| text == value)
        .unwrap_or_else(|| panic!("{value:?} is on screen: {:?}", d.texts()));
    let point = Point::new(at.x + at.width - 1.0, at.y + at.height * 0.5);
    d.press_mouse(point);
    d.run(0.02);
    d.release_mouse(point);
    d.run(0.3);
    press(&mut d, KeyStroke::new(ShortcutKey::End));
    d
}

fn press(d: &mut Driver<Form>, key: KeyStroke) {
    d.key(key);
    d.run(0.05);
}

fn s() -> KeyStroke {
    KeyStroke::new(ShortcutKey::Char('s'))
}

/// **One binding, `primary+S`, is Ctrl+S on a keyboard that is not Apple's** — and the logo
/// key with S is not it there.
#[test]
fn a_primary_binding_is_ctrl_off_an_apple_keyboard() {
    let mut d = open("draft", false);
    press(&mut d, s().meta());
    assert_eq!(d.app().saves, 0, "the logo key + S is not Save here");
    press(&mut d, s().ctrl());
    assert_eq!(d.app().saves, 1, "Ctrl+S is");
    press(&mut d, s().primary());
    assert_eq!(
        d.app().saves,
        2,
        "and a stroke written primary is pressed as Ctrl"
    );
}

/// **The same binding is Cmd+S on an Apple keyboard**, where Ctrl+S is not Save.
#[test]
fn a_primary_binding_is_cmd_on_an_apple_keyboard() {
    let mut d = open("draft", true);
    press(&mut d, s().ctrl());
    assert_eq!(d.app().saves, 0, "Ctrl+S is not Save on a Mac");
    press(&mut d, s().meta());
    assert_eq!(d.app().saves, 1, "Cmd+S is");
    press(&mut d, s().primary());
    assert_eq!(
        d.app().saves,
        2,
        "and a stroke written primary is pressed as Cmd"
    );
}

/// **An application rebinds the framework's editing keys**: another key to its Undo, and
/// `Intent::DO_NOTHING` on Select all, which takes the key from the field — on both
/// keyboards. The defaults it did not touch, Undo on primary+Z, are still there.
#[test]
fn an_application_rebinds_and_switches_off_editing_keys() {
    for apple in [false, true] {
        let mut d = open("one", apple);
        press(&mut d, KeyStroke::new(ShortcutKey::Char('!')));
        assert_eq!(d.app().value, "one!", "typed");

        press(&mut d, KeyStroke::new(ShortcutKey::Char('u')).primary());
        assert_eq!(
            d.app().value,
            "one",
            "primary+U is the field's Undo (apple {apple})"
        );
        press(
            &mut d,
            KeyStroke::new(ShortcutKey::Char('z')).primary().shift(),
        );
        assert_eq!(
            d.app().value,
            "one!",
            "primary+Shift+Z is still Redo (apple {apple})"
        );
        press(&mut d, KeyStroke::new(ShortcutKey::Char('z')).primary());
        assert_eq!(
            d.app().value,
            "one",
            "primary+Z is still Undo (apple {apple})"
        );

        // Select all is switched off: typing after it adds to the text rather than
        // replacing a selection.
        press(&mut d, KeyStroke::new(ShortcutKey::Char('a')).primary());
        press(&mut d, KeyStroke::new(ShortcutKey::Char('?')));
        assert_eq!(
            d.app().value,
            "one?",
            "primary+A did nothing (apple {apple})"
        );
    }
}

/// **Redo is Ctrl+Y off an Apple keyboard, and Cmd+Y is not Redo on one** — the default
/// table, through the shell.
#[test]
fn ctrl_y_redoes_off_an_apple_keyboard_only() {
    let y = KeyStroke::new(ShortcutKey::Char('y'));
    let z = KeyStroke::new(ShortcutKey::Char('z'));

    let mut d = open("one", false);
    press(&mut d, KeyStroke::new(ShortcutKey::Char('!')));
    press(&mut d, z.ctrl());
    assert_eq!(d.app().value, "one");
    press(&mut d, y.ctrl());
    assert_eq!(d.app().value, "one!", "Ctrl+Y is Redo");

    let mut d = open("one", true);
    press(&mut d, KeyStroke::new(ShortcutKey::Char('!')));
    press(&mut d, z.meta());
    assert_eq!(d.app().value, "one");
    press(&mut d, y.meta());
    assert_eq!(d.app().value, "one", "Cmd+Y is not Redo on a Mac");
}
