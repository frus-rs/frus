// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Word and line deletion in a field, driven through the shell (milestone 849, L-038): the
//! keys as a keyboard sends them, with their modifiers held, reach the field as a word or a
//! line deleted, and one undo brings it back.

use crate::app::testing::Driver;
use crate::{Application, Command};
use frus_widgets::{Container, Flex, KeyStroke, Point, ShortcutKey, TextField, Theme, Widget};

const W: f32 = 400.0;
const H: f32 = 200.0;

/// One single-line field.
struct Form {
    value: String,
}

impl Application for Form {
    type Message = String;

    fn update(&mut self, value: String) -> Command<String> {
        self.value = value;
        Command::none()
    }

    fn view(&self, _theme: &Theme) -> Box<dyn Widget<String>> {
        let field = TextField::new(self.value.clone())
            .on_input(|value| value)
            .width(W - 40.0);
        Box::new(
            Container::new()
                .width(W)
                .height(H)
                .child(Flex::column().child(field)),
        )
    }
}

/// The field holding `value`, focused, its caret at the end of the text.
fn open(value: &str, apple: bool) -> Driver<Form> {
    let mut d = Driver::new(
        Form {
            value: value.into(),
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

/// **Ctrl+Backspace deletes the word before the caret, and Ctrl+Z brings it back whole**:
/// two words deleted are two undos, and each brings back one whole word. Before, Ctrl+Backspace
/// deleted one character.
#[test]
fn ctrl_backspace_deletes_a_word_and_ctrl_z_brings_it_back() {
    let mut d = open("one two three", false);
    let backspace = KeyStroke::new(ShortcutKey::Backspace);
    press(&mut d, backspace.ctrl());
    assert_eq!(d.app().value, "one two ");
    press(&mut d, backspace.ctrl());
    assert_eq!(d.app().value, "one ");
    let z = KeyStroke::new(ShortcutKey::Char('z'));
    press(&mut d, z.ctrl());
    assert_eq!(d.app().value, "one two ", "one undo, one word");
    press(&mut d, z.ctrl());
    assert_eq!(d.app().value, "one two three");
    // Ctrl+Delete from the start of the field: the first word.
    press(&mut d, KeyStroke::new(ShortcutKey::Home));
    press(&mut d, KeyStroke::new(ShortcutKey::Delete).ctrl());
    assert_eq!(d.app().value, " two three");
    // Alt+Backspace from the end: the whole line.
    press(&mut d, KeyStroke::new(ShortcutKey::End));
    press(&mut d, backspace.alt());
    assert_eq!(d.app().value, "");
}

/// **On an Apple keyboard, Option+Backspace deletes a word and Cmd+Backspace the line**,
/// and Ctrl+Backspace deletes one character, as Backspace does there.
#[test]
fn option_and_cmd_backspace_on_an_apple_keyboard() {
    let mut d = open("one two three", true);
    let backspace = KeyStroke::new(ShortcutKey::Backspace);
    press(&mut d, backspace.ctrl());
    assert_eq!(d.app().value, "one two thre");
    press(&mut d, backspace.alt());
    assert_eq!(d.app().value, "one two ");
    press(&mut d, backspace.meta());
    assert_eq!(d.app().value, "");
    press(&mut d, KeyStroke::new(ShortcutKey::Char('z')).meta());
    assert_eq!(d.app().value, "one two ", "Cmd+Z brings the line back");
}
