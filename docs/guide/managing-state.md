# Managing state

frus has one recommended way to hold the state of an application, and it scales from a counter to
an application with many screens. This page gives that way first, then says when each of the
other tools is the right one.

The short version:

| What the state is | Where it lives | How it is read |
| --- | --- | --- |
| What one component shows and nobody else needs (a field's draft, an open menu) | in the component: `cx.use_state`, or a `StatefulWidget` | by the component itself |
| What a part of the screen shares (a cart, a filter, a form being filled in) | a `Model<T>`, owned by the component the part starts at, put in scope with `Scoped` | `cx.watch`, `cx.select`, `cx.read` in the components below |
| What the whole application shares (the signed-in user, the settings) | a `Model<T>` put in scope at the root | the same |
| What a widget is driven by (a text field's text, a scroll position) | a controller (`TextEditingController`, `ScrollController`, …) | the widget it is given to |

A change builds again **only the components that read what changed**. Nothing else is built.

## One component: `use_state`

State lives as low as it can. A value that one component shows is kept by that component:

```rust
use frus::{component, Button, Column, Text, Widget};

let counter = component(|cx| {
    let count = cx.use_state(|| 0);
    let add = count.clone();
    Box::new(
        Column::new()
            .child(Text::new(format!("{}", count.get())))
            .child(Button::new("+").on_press(move || add.update(|n| *n += 1))),
    ) as Box<dyn Widget>
});
# let _ = counter;
```

A change to `count` builds this component again, where it stands, and nothing around it. A
`StatefulWidget` does the same with a struct, and adds a lifecycle (`init_state`,
`did_update_widget`, `dispose`): see [Components and state](components.md).

## A part of the screen: a `Model` in scope

When several components need the same value, it becomes a `Model<T>`: a value that knows who
reads it. The component where the shared part of the screen starts owns it (`cx.use_model`) and
puts it **in scope** for everything below with `Scoped`. The components below find it by its type:

- `cx.watch::<T>()` reads the whole model. The component is built again on every change.
- `cx.select(|t: &T| part)` reads one part. The component is built again only when that part
  changes.
- `cx.read::<T>()` reads it without depending on it: for a handler that changes it. The
  component is never built again because of the model.

```rust
use frus::{component, BuildContext, Button, Column, Model, Scoped, Text, Widget};

#[derive(Default)]
pub struct Cart {
    pub items: Vec<String>,
    pub note: String,
}

impl Cart {
    pub fn add(&mut self, item: &str) {
        self.items.push(item.to_string());
    }
}

/// Every change to the cart builds the lines again.
fn lines(cx: &BuildContext) -> Box<dyn Widget> {
    let cart = cx.watch::<Cart>();
    Box::new(Text::new(cart.with(|cart| cart.items.join(", "))))
}

/// Only a change in the number of items builds the badge again: a new note does not.
fn badge(cx: &BuildContext) -> Box<dyn Widget> {
    let count = cx.select(|cart: &Cart| cart.items.len());
    Box::new(Text::new(format!("{count} items")))
}

/// The button changes the cart and never needs to be built again because of it.
fn add_pear(cx: &BuildContext) -> Box<dyn Widget> {
    let cart = cx.read::<Cart>();
    Box::new(Button::new("Add a pear").on_press(move || cart.update(|cart| cart.add("pear"))))
}

/// The shop owns its cart and puts it in scope for what it shows.
fn shop(cx: &BuildContext) -> Box<dyn Widget> {
    let cart = cx.use_model(Cart::default);
    Box::new(Scoped::new(
        cart,
        Column::new()
            .child(component(badge))
            .child(component(lines))
            .child(component(add_pear)),
    ))
}

// Two shops, two carts: each one's components find the nearest cart, their own.
let page = Column::new().child(component(shop)).child(component(shop));
# let _ = page;
```

Pressing *Add a pear* in the first shop builds that shop's `lines` and `badge`, and nothing else:
not the shop, not its button, nothing of the second shop. The test of this lives in frus itself
(`crates/frus-shell/src/scoped_tests.rs`), with a counter of builds.

The rules that make this work:

- **The nearest one wins.** A component finds the `Model<T>` of the closest `Scoped` above it.
  Two instances of the same screen each put their own model in scope, and do not see each
  other's.
- **The owner keeps it.** `cx.use_model` makes the model the first time and keeps it as long as
  the component is in the tree. A model made inside a build, `Scoped::new(Model::new(..), ..)`,
  is a new one on every build of that component, with what it held lost.
- **Reading is what subscribes.** There is no listener to add or remove: a component that read the
  model in its last build is built again when it changes, and one that stopped reading it is not.
- **Change it through the model.** `update` and `set` tell the readers; changing the value behind
  their back does not. `set` tells nobody when the new value equals the old one.
- **Keep the logic in the value.** `Cart::add` is a plain method, tested without a window. The
  components only call it.

## The whole application: a `Model` at the root

What every screen needs goes in scope at the root, the same way:

```rust
use frus::{component, FrusApp, Model, Scoped, Text, Widget};

#[derive(Default)]
pub struct Session {
    pub user: Option<String>,
}

let session = Model::new(Session::default());
let app = FrusApp::from_fn(move |_cx| {
    let greeting = component(|cx| {
        let user = cx.select(|s: &Session| s.user.clone());
        Box::new(Text::new(match user {
            Some(name) => format!("Hello, {name}"),
            None => "Signed out".into(),
        })) as Box<dyn Widget>
    });
    Box::new(Scoped::new(session.clone(), greeting)) as Box<dyn Widget>
});
# let _ = app;
```

The model is made once, outside the function that builds the screen, so a rebuild of the whole
tree (the theme changed, the window was resized) keeps it. An object the application reaches from
everywhere and that never changes, such as a repository or a client for a server, can be put in
scope the same way, as a plain value: `cx.service::<Repository>()` finds it, and depends on
nothing.

`cx.runtime().states.provide(Rc::new(value))` still works: what it provides is the outermost
scope, found when no `Scoped` above has a value of the type.

## Dialogs, sheets, menus and pages

An overlay sees what is in scope **where it is declared**. A dialog, a bottom sheet, a drawer, a
popover, a hover card, a menu's rows, a dropdown's options, a search view's suggestions, an open
container: each is declared in the tree by the page that opens it, and drawn above everything
after the page. A component inside it finds the page's `Model` exactly as a component of the page
does, the nearest one first, and is built again, alone, when the model changes. It also sees that
place's theme.

```rust
use frus::{component, BuildContext, Dialog, Scoped, Text, Widget};

#[derive(Default)]
pub struct Cart {
    pub items: u32,
}

let page = component(|cx: &BuildContext| {
    let cart = cx.use_model(Cart::default);
    // The dialog's lines read the page's cart: nothing is passed to them.
    let lines = component(|cx: &BuildContext| {
        let n = cx.select(|c: &Cart| c.items);
        Box::new(Text::new(format!("{n} in the cart"))) as Box<dyn Widget>
    });
    Box::new(Scoped::new(
        cart,
        Dialog::new(true).child(lines).body(Text::new("The page")),
    )) as Box<dyn Widget>
});
# let _ = page;
```

A **page pushed on the router** is another matter: it is not inside the page that pushed it, but
beside it, under the router. It sees what is in scope above the router (put it there with
`FrusApp::builder(|_, pages| Box::new(Scoped::new(session.clone(), pages)))`) and not what the
page that pushed it put in scope. A router inside a page (a shell's branch) is inside that page:
its pages see what that page put in scope.

To take a value somewhere it is not in scope (a page pushed on the router, a snack bar, whose
words are shown by the application's root), hand it over: wrap what is shown in
`Scoped::new(model.clone(), ..)`, or let a button's closure capture the model. A `Model` is a
cheap handle: every clone is the same model, and its readers are told of a change made through
any of them.

## How far a change reaches

frus keeps the tree it built and builds again only what was marked:

- a component whose own state changed (`use_state`, `set_state`);
- a component that read a `Model` that changed, and, for a `select`, whose part changed.

A component built again builds what it returns, so the components inside it are built too. Keep
the readers small and low in the tree, and the part above them that only passes the model down
is never built by its changes. A component that reads nothing is not built by a change.

Some changes still build the whole tree: the theme, the window's size, the language, a hot
reload, and anything that calls `request_rebuild`, such as `ValueNotifier` and `ChangeNotifier`.
Every component is then built again, every state and every model is kept.

## The other tools, and when to use them

**`Application` with `update` and `view`.** An application can be written as one state struct, a
`Message` enum and an `update` that is the only place state changes. Its `view` reads the whole
state, so every message builds the whole tree. Choose it when the logic is worth testing as a pure
function of its messages, message by message. The two mix: the `view` can hold components, and a
`Model` the application owns can be put in scope in the `view`, so that what the components change
between messages builds only their readers.

**`StatefulWidget` and hooks.** For the state of one component, as above. A `StatefulWidget`
when the state deserves a struct or needs a lifecycle; hooks (`use_state`, `use_ref`, `use_memo`,
`use_effect`) in a function component otherwise.

**`ValueNotifier` and `ChangeNotifier`.** A value, or an object of your own, that something
outside the tree listens to with `add_listener`. A change rebuilds the whole tree, which is what
code written before `Model` expects. For state the screen shows, prefer a `Model`: it is a
`Listenable` too, so `add_listener` works on it, and a change builds only its readers.

**Controllers.** A `TextEditingController`, a `ScrollController`, an `AnimationController`: the
state a widget is driven by, for when the program needs to read or move it. They are kept by the
component that owns the widget (`cx.use_text_controller`, …).

**Values passed down.** A value one child needs is simply given to it as configuration. Scope is
for values many components below need, at depths that do not know about each other.

## A real application, laid out

The same pattern, file by file (see [Structuring an application](../app-structure.md)):

- `models/`: the values (`Cart`, `Session`) and their methods, tested in `tests/`.
- the page that owns a model: `let cart = cx.use_model(Cart::default)` and
  `Scoped::new(cart, ..)` around its body.
- the widgets of the page: `cx.watch`, `cx.select` or `cx.read`, nothing passed through the
  constructors in between.
- the root: `Scoped::new(session.clone(), router)` for what every page needs.

The gallery's *Scoped models* demo (`frus-examples/widgets`, under State) shows two carts side by
side, with the number of times each part was built.
