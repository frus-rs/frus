//! Scoped models — `Model`, `Scoped`, and `cx.watch` / `cx.select` / `cx.read`: state shared by
//! a part of the screen, with each instance of that part holding its own.
//!
//! Two carts side by side. Each cart component owns a `Model<Cart>` (`cx.use_model`) and puts it
//! in scope with `Scoped`; the components below find the nearest one by its type. A change to
//! one cart builds again only the components that read it: every part shows how many times it
//! has been built, so a press shows which parts it reached.

use frus::{
    component, Align, BuildContext, Button, Column, Component, Row, Scoped, Text, Variant, Widget,
};

use crate::common::shared::{section, sections};

/// What a cart holds. Its methods are the only way it changes, and are tested on their own.
#[derive(Default, Clone, PartialEq)]
pub struct Cart {
    pub items: Vec<String>,
    pub note: String,
}

impl Cart {
    pub fn add(&mut self, item: &str) {
        self.items.push(item.to_string());
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    pub fn sign(&mut self) {
        self.note.push('*');
    }
}

pub fn example() -> Box<dyn Widget> {
    sections(vec![Box::new(section(
        "Two carts, each with its own model",
        Row::new()
            .spacing(32.0)
            .cross_axis_alignment(Align::Start)
            .child(cart("A"))
            .child(cart("B")),
    ))])
}

/// How many times the component calling it has been built, this build included.
fn builds(cx: &BuildContext) -> u32 {
    let built = cx.use_ref(|| 0_u32);
    built.update(|n| *n += 1);
    built.get()
}

/// One cart: it owns its model and puts it in scope. It never reads the model itself, so the
/// model's changes never build it.
pub fn cart(name: &'static str) -> Component {
    component(move |cx| {
        let built = builds(cx);
        let cart = cx.use_model(Cart::default);
        Box::new(Scoped::new(
            cart,
            Column::new()
                .spacing(8.0)
                .cross_axis_alignment(Align::Start)
                .child(Text::new(format!("Cart {name} (built {built})")).heading())
                .child(lines(name))
                .child(badge(name))
                .child(buttons(name)),
        ))
    })
}

/// Watches the whole cart: built again on every change to it.
fn lines(name: &'static str) -> Component {
    component(move |cx| {
        let built = builds(cx);
        let cart = cx.watch::<Cart>();
        let shown = cart.with(|cart| {
            if cart.items.is_empty() {
                "empty".to_string()
            } else {
                cart.items.join(", ")
            }
        });
        let note = cart.with(|cart| cart.note.clone());
        Box::new(Text::new(format!(
            "{name} lines: {shown}, note '{note}' (built {built})"
        )))
    })
}

/// Selects the number of items: a new note does not build it.
fn badge(name: &'static str) -> Component {
    component(move |cx| {
        let built = builds(cx);
        let count = cx.select(|cart: &Cart| cart.total());
        Box::new(Text::new(format!(
            "{name} badge: {count} items (built {built})"
        )))
    })
}

/// Only changes the cart: `read` depends on nothing, so the cart never builds it.
fn buttons(name: &'static str) -> Component {
    component(move |cx| {
        let built = builds(cx);
        let (add, sign) = (cx.read::<Cart>(), cx.read::<Cart>());
        Box::new(
            Column::new()
                .spacing(4.0)
                .cross_axis_alignment(Align::Start)
                .child(
                    Button::new(format!("Add tea to {name}"))
                        .on_press(move || add.update(|cart| cart.add("tea"))),
                )
                .child(
                    Button::new(format!("Sign the note of {name}"))
                        .variant(Variant::Text)
                        .on_press(move || sign.update(Cart::sign)),
                )
                .child(Text::new(format!("{name} buttons (built {built})"))),
        )
    })
}
