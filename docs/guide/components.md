# Components and state

A frus screen is a tree of **components**: widgets that say what they build, and keep what they
have to remember between builds. When state changes, frus builds the tree again on the next
frame, and each component finds its state where it left it: at its place in the tree.
This page covers the three kinds of component, how state is kept and shared, keys, `async` work,
and keeping state across a restart. [Starting a frus application](../getting-started.md) has the
short version.

## A function: `component` and hooks

The smallest component is a function of a `BuildContext`. Its memory is **hooks**: `use_state`
keeps a value and builds again when it is set.

```rust
use frus::{button, column, component, text, BuildContext, Widget};

fn counter(cx: &BuildContext) -> Box<dyn Widget> {
    let count = cx.use_state(|| 0);
    let add = count.clone();
    Box::new(column![
        text(format!("{}", count.get())).size(48.0),
        button("+", move || add.update(|n| *n += 1)),
    ])
}

// A function is a component wherever a widget goes...
let page = column![component(counter), component(counter)];
// ...and each one keeps its own count.
# let _ = page;
```

| Hook | What it keeps |
| --- | --- |
| `use_state(init)` | a value; `set` and `update` build again |
| `use_ref(init)` | a value that changes nothing on screen when set (a cache, a handle) |
| `use_memo(deps, compute)` | a value worked out again only when `deps` changes |
| `use_effect(deps, effect)` | work done after a build when `deps` changes, with an optional clean-up |
| `use_interval(period, tick)` | a timer, stopped when the component leaves |
| `use_text_controller(text)` | a text field's text, see [Taking input](input.md) |
| `use_animation_controller(init)` | an animation, see [Animation](animation.md) |
| `use_future(deps, make)`, `use_stream(deps, make)` | the latest result of `async` work |
| `use_pref(key, default)` | a value kept on the device, see [The system](platform.md#preferences) |
| `use_restorable_state(id, init)` | a value that survives the system ending the process |

Hooks are told apart by the **order** they are called in. Call them at the top of the function,
the same ones in the same order on every build, never inside an `if` or a loop. (`use_interval`,
`use_pref` and `use_connectivity` are the exceptions: they keep no place and may be conditional.)

```rust
use frus::{text, BuildContext, Widget};

fn search(cx: &BuildContext) -> Box<dyn Widget> {
    let query = cx.use_state(String::new);
    let words = query.get();
    // Worked out again only when the query changes.
    let matches = cx.use_memo(words.clone(), move || {
        ["apple", "apricot", "banana"].iter().filter(|w| w.starts_with(&words)).count()
    });
    // Runs once, after the first build; the closure it returns runs when the component leaves.
    cx.use_effect((), || {
        frus::log::info!("search opened");
        || frus::log::info!("search closed")
    });
    Box::new(text(format!("{} matches", matches)))
}
# let _ = search;
```

## Configuration without memory: `StatelessWidget`

A struct is a component when it implements `StatelessWidget`: its fields are what the parent
configures, and `build` says what it looks like. It may still call hooks.

```rust
use frus::{column, text, BuildContext, StatelessWidget, Widget};

struct Price {
    amount: f64,
    currency: &'static str,
}

impl StatelessWidget for Price {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        Box::new(text(format!("{:.2} {}", self.amount, self.currency)).size(20.0))
    }
}

let prices = column![
    Price { amount: 4.5, currency: "EUR" }.into_widget(),
    Price { amount: 12.0, currency: "USD" }.into_widget(),
];
# let _ = prices;
```

## A state with a lifecycle: `StatefulWidget`

When the state deserves a struct of its own, or needs to set something up and tear it down, the
widget is split in two: the `StatefulWidget` is the configuration, made again on every build of
its parent; its `State` is made once, and kept as long as the widget stays at its place.

```rust
use frus::{column, text, Checkbox, State, StateContext, StatefulWidget, Widget};
use std::time::Duration;

/// A countdown that starts again when its parent gives it another length.
struct Countdown {
    seconds: u32,
}

struct CountdownState {
    left: u32,
    paused: bool,
}

impl StatefulWidget for Countdown {
    type State = CountdownState;
    fn create_state(&self) -> CountdownState {
        CountdownState { left: self.seconds, paused: false }
    }
}

impl State for CountdownState {
    type Widget = Countdown;

    // The parent built `Countdown` again, with another configuration.
    fn did_update_widget(&mut self, old: &Countdown, cx: &StateContext<Self>) {
        let seconds = cx.widget().seconds;
        if seconds != old.seconds {
            self.left = seconds;
        }
    }

    fn dispose(&mut self) {
        frus::log::debug!("countdown gone with {} seconds left", self.left);
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        if !self.paused && self.left > 0 {
            // A state's context is a BuildContext too: hooks work here.
            cx.use_interval(Duration::from_secs(1), cx.callback(|s: &mut Self| s.left -= 1));
        }
        Box::new(column![
            text(format!("{} s", self.left)).size(32.0),
            Checkbox::new(self.paused)
                .label("Paused")
                .on_toggle(cx.handler(|s: &mut Self, paused: bool| s.paused = paused)),
        ])
    }
}

let timer = Countdown { seconds: 30 }.into_widget();
# let _ = timer;
```

- `cx.callback(|state| ..)` is a handler that changes the state and builds again;
  `cx.handler(|state, value| ..)` does the same with the value a control reports.
- `cx.handle()` gives a `StateHandle` that a timer, a listener or a task can keep, and call
  `set_state` on later. After the widget has left, `set_state` does nothing.
- `init_state` runs once before the first build, `did_update_widget` when the parent builds the
  widget again, `dispose` when it leaves.
- Changing the state from inside `build` is refused: a build that changes what it builds from
  never settles.

## Sharing state between components

State lives as low as it can: in the component that shows it. When two parts of a screen need
the same value, it moves up to their closest common parent, which hands it down as
configuration. A value that many components read (the signed-in user, a cart) is a `Model` put in
scope with `Scoped`, and the components below read it with `cx.watch`, `cx.select` or `cx.read`:
a change builds again only the ones that read it. [Managing state](managing-state.md) is the
whole story, and the place to start.

A `ValueNotifier` is the older way, still there for what listens from outside the tree: a change
to it builds the whole tree.

```rust
use frus::{column, text, Button, ValueListenableBuilder, ValueNotifier, Widget};
use std::rc::Rc;

/// What the whole application shares.
struct Cart {
    items: ValueNotifier<Vec<String>>,
}

fn badge(cart: Rc<Cart>) -> Box<dyn Widget> {
    // Built again whenever the items change.
    Box::new(ValueListenableBuilder::new(cart.items.clone(), |_cx, items: &Vec<String>| {
        Box::new(text(format!("{} in the cart", items.len())))
    }))
}

fn shop(cart: Rc<Cart>) -> Box<dyn Widget> {
    let add = cart.clone();
    Box::new(column![
        badge(cart.clone()),
        Button::new("Add a pear").on_press(move || add.items.update(|items| items.push("pear".into()))),
    ])
}

let cart = Rc::new(Cart { items: ValueNotifier::new(Vec::new()) });
let page = shop(cart.clone());
cart.items.update(|items| items.push("apple".into()));
assert_eq!(cart.items.with(|items| items.len()), 1);
# let _ = page;
```

`ChangeNotifier` is the same without a value, for an object of your own that says "something
changed"; `ListenableBuilder` builds from it. `add_listener` hears a change outside the tree, and
listening stops when the returned subscription is dropped.

## Keys: which state is which

frus matches a component to its state by its **place** among its parent's children. When
children are reordered, inserted or removed, give each a key from your data, so that its state
follows it rather than staying at the old place:

```rust
use frus::{component, keyed, text, Column, Widget};

struct Task {
    id: u64,
    title: String,
}

fn tasks(list: &[Task]) -> Box<dyn Widget> {
    let mut rows = Column::new();
    for task in list {
        let title = task.title.clone();
        // Each row keeps its own state, whatever its position.
        rows = rows.child(keyed(task.id, component(move |cx| {
            let done = cx.use_state(|| false);
            Box::new(text(format!("{} {}", if done.get() { "✓" } else { "·" }, title)))
        })));
    }
    Box::new(rows)
}
# let _ = tasks;
```

`ListView` and `GridView` rows are keyed by their index unless you give them keys; a list whose
rows move should. A key also names a widget for `frus::host::focus(key)` and
`frus::host::scroll_to(key, ..)`.

## Work that waits

A handler that waits for the network or a file is `async`. It runs on the interface's thread,
between frames, so it may change state directly:

```rust
use frus::{column, Button, State, StateContext, StatefulWidget, Text, Widget};

async fn fetch_quote() -> String {
    String::from("Simple is better than complex.")
}

struct Quote;
#[derive(Default)]
struct QuoteState {
    text: String,
    loading: bool,
}

impl StatefulWidget for Quote {
    type State = QuoteState;
    fn create_state(&self) -> QuoteState {
        QuoteState::default()
    }
}

impl State for QuoteState {
    type Widget = Quote;
    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let load = cx.async_callback(|state| async move {
            state.set_state(|s| s.loading = true);
            let quote = fetch_quote().await;
            state.set_state(|s| {
                s.text = quote;
                s.loading = false;
            });
        });
        Box::new(column![
            Text::new(self.text.clone()).wrap(),
            Button::new("New quote").enabled(!self.loading).on_press(load),
        ])
    }
}
# let _ = Quote;
```

A task belongs to its component: when the component leaves, the task is dropped at its next
`await`. A screen's data that must load before it can show anything is a `use_future` or a
`FutureBuilder`; see [Talking to a server](network.md).

## Keeping state when the process ends

A phone ends an application in the background when it runs short of memory, and the reader
expects to come back to where they were. Turn restoration on, then say what each component keeps:

```rust
use frus::{text, BuildContext, FrusApp, GoRoute, GoRouter, Widget};

fn tabs(cx: &BuildContext) -> Box<dyn Widget> {
    // Kept under "tab": the same value comes back after the process is started again.
    let tab = cx.use_restorable_state("tab", || 0usize);
    Box::new(text(format!("tab {}", tab.get())))
}

fn app() -> FrusApp {
    let router = GoRouter::new(vec![GoRoute::new("/", |_, _| frus::component(tabs))]);
    // The router's pages come back on their own; each page keeps what it registers.
    FrusApp::router(router).restoration_scope_id("app")
}
# let _ = app;
```

`use_restorable_text_controller`, a `TextField`'s or a `ListView`'s `restoration_id`, and a
`State` with a `restoration_id` and `save`/`restore` do the same for a field's text, a list's
position and a state of your own.

The same `save` and `restore` keep a `State` across a reload under `frus run` (`r`): see
[Running and hot reload](running.md).

## Typed messages: `Application`

`FrusApp` with components is how most applications are written. An application can instead
implement `Application`: one `Message` type, an `update(&mut self, message)` that is the only
place state changes, and a `view(&self, theme)` that builds the screen from it. All the logic is
then a pure function that a test drives message by message, and the `Application` trait has
hooks `FrusApp` does not offer: `on_lifecycle` (resumed, paused), `accessibility`,
`high_contrast_theme`, `resolved_locale`. The two mix: components can sit anywhere inside a
`view`. See the `Application` trait in the API reference.
