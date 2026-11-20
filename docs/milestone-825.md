# Milestone 825 — State scoped to a subtree, and only its readers built again

## Objective

Issue #222: an application could share state down the tree in one way only, and that way was
global. `provide<T>(Rc<T>)` / `service<T>()` kept one value per type for the whole application,
so a second screen of the same kind providing its own cart overwrote the first. And whatever was
shared could not say who read it: a `ValueNotifier` change built the whole tree.

The issue is done when an example provides two independent instances of a model to two subtrees,
a change in one builds only its readers (shown by a counter in a test), and the docs have a single
"managing state" guide.

## Design

### A value in scope: `Scoped`

`Scoped::new(value, child)` is a transparent wrapper that puts `value` in scope for its subtree.
A component below finds the **nearest** value of a type. Two instances of a screen each wrap
their body in their own `Scoped`, and each one's components find their own.

How the components find it: the walks that can build a component hold the values in scope as
they go down, the same way they already hold the scoped surface, the shell and the scroll
behaviour. A new `Widget` hook, `scoped_values(&self, out)`, says what a node puts in scope;
only `Scoped` answers, and every transparent wrapper forwards it (it sits with the forwarded
`expand` in the transparent macro, so a wrapper written with the macro cannot forget it). The
stack lives in the `StateStore`, not in a thread-local, and is held:

- by the walk that builds components before the layout (`build_deferred`);
- by the layout walk, so a deferred build finds the values above it;
- by the paint walk, where a list built on demand builds the rows in view: without it, a row's
  component found no model. The shell test with a `ListView` in each cart caught this (the
  unit tests, which build without laying out or painting, could not);
- by the relayout fingerprint walk, which may run a deferred build first;
- by `Scoped::expand` itself, for a component that the walk reaches *through* the `Scoped`
  (a component that returns `Scoped::new(model, OtherComponent)`: the two are one node, and
  the inner component is built while the walk is still at it).

`provide` / `service` keep working: what `StateStore::provide` holds is the outermost scope,
found when no `Scoped` above has the type. `BuildContext::service` now looks in scope first.

### A value that knows its readers: `Model<T>`

`Model<T>` holds a value and the components that read it. A component reads it in one of three
ways:

- `cx.watch::<T>()`: the whole model. Any change builds the component again.
- `cx.select(|t: &T| part)`: one part (`PartialEq + Clone`). The component is built again only
  when that part changes; the selector is run again on each change and compared with what the
  component saw.
- `cx.read::<T>()`: no dependency, for a handler that changes it.

`cx.use_model(init)` keeps a model for as long as the component that made it, which is how the
owner of a part of the screen makes the model it puts in scope.

A reading component is registered by its **mark**, the cell its own state sets since milestone
822. When the model changes (`update`, or `set` with a different value), each reader it concerns
is marked and `request_component_rebuild` is called, so the next frame is a frame of marked
components: the tree kept from the last frame, with those components built again where they
stand. Nothing else is built: not the component that owns the model, not the one that put it in
scope, not a component that only `read` it, nothing of another instance.

A dependency holds for the build that made it. A marked reader is forgotten, and registers again
when its new build reads the model. A reader built again for another reason (its own state) that
did not read the model this time is recognised by the rebuild its hooks were last built in, and is
not marked. A reader that has left the tree is dropped (the dependency holds the mark weakly).

`Model<T>` is also a `Listenable`: `add_listener` hears every change, without asking for a
rebuild of the tree. `ChangeNotifier` gained a crate-private `tell_listeners` for that;
`notify_listeners` is unchanged and still asks for the whole tree.

### With the Elm-style `Application`

An `Application`'s messages build the whole tree (`messages_rebuild_view` answers `true`), as
before: its `view` reads the state `update` changed. A `Model` it owns can be put in scope in its
`view`; a message still builds everything, and what the components below change in the model
between messages builds only their readers. `FrusApp` (whose messages are closures) answers
`false`, so a handler that only changes a model builds the model's readers.

### With milestone 822's memoization

Milestone 822 keeps the tree and builds again only the components marked by their own state. A
model's change is one more way to mark a component: the same mark, the same frame. A component
kept is not built and keeps the dependencies of its last build; a component built again builds
what it returns, and the readers inside it register again as they are built. Everything that
still builds the whole tree (the theme, the surface, the locale, a hot patch, `request_rebuild`)
builds every reader too, and keeps every model, which lives in a hook, a `State` or the
application.

## Alternatives rejected

- **Scopes keyed by the component that provides them**, with readers finding them through a
  parent chain. Components do not know their parents: the tree is a tree of widgets, walked from
  the root, and the walk already carries the other inherited things (theme, surface, shell). One
  more inherited thing in the walk follows the same rules and the same corner cases.
- **Making any `T` reactive** (`Scoped::new(value)` and a compare of the old value with the new
  one). The framework would have to keep a copy of every shared value and compare them on every
  frame; a change made through a shared pointer would not be seen at all. A `Model` is told of
  every change, and knows exactly who to tell.
- **Reusing `ValueNotifier`** as the scoped model. Its change asks for the whole tree, and code
  written for it listens and rebuilds by hand; turning that into "readers only" would change what
  existing applications see. It stays, and the guide says when it is still the right tool.
- **Long-lived subscriptions** (a listener per reader, dropped when the reader is disposed).
  They outlive the build that read the value: a component that read a model under an `if` would
  keep being built after it stopped. Registering per build is what makes "reading is what
  subscribes" true.

## The guide

`docs/guide/managing-state.md`: one recommended pattern (state in the component, a `Model` in
scope for a part of the screen or the whole application, logic in the model's methods), how far a
change reaches, and when to use each other tool: `Application` with `update`/`view`,
`StatefulWidget` and hooks, `ValueNotifier`/`ChangeNotifier`, controllers, values passed down.
Its examples are compiled and run as doctests (`frus`'s `GuideManagingState`). The guide index and
the "Sharing state" section of `components.md` point to it.

## Example

The gallery (`frus-examples/widgets`) has a new demo under State, **Model & Scoped**: two carts
side by side, each owning its model, with every part showing how many times it has been built.
Its test (`tests/scoped_models_test.rs`, through `frus::Tester`) adds to one cart and checks that
only that cart's lines and badge were built again, and that signing a note builds the lines and
not the badge. The gallery's `Cargo.toml` gained a dev-dependency on frus with `testing`.

The test compares each part's count before and after a press rather than expecting 1: the
tester's opening `settle` draws two frames that each build the whole page, so every part starts
at 2. A first version asserted 1 and failed on that alone, while the counts after the press were
already right.

## Tests

- `frus-widgets` (`scoped.rs`): two instances, each with its own model, and a change to one
  builds only its readers (counted per part); a selector builds only when its part changes; a
  change nobody reads asks for no frame; the nearest scope wins, a component built directly by a
  `Scoped` finds it, and the application's value is the outermost; a component that stops reading
  stops being built; watching the whole outweighs a selected part; a listener hears changes with
  no rebuild of the tree; a missing model panics with how to provide one.
- `frus-shell` (`scoped_tests.rs`, through the shell's frame and `Driver`, with real taps): a
  press in one of two carts builds that cart's lines and badge once each, and neither the screen,
  that cart's panel and buttons, nor anything of the other cart; the rows of a `ListView`
  built on demand in each cart see their own cart; a note builds the lines and not the badge.

## Not done

- **Overlays are not covered by a test.** A dialog, a sheet or a menu built where the walk
  reaches it below the `Scoped` sees the scope; one the shell places above the page does not.
  Which overlays are which was not checked one by one. Until then the guide's advice holds: pass
  the model to what an overlay shows, or put it in scope at the root.
- **`Application` messages still build the whole tree.** An Elm-style application that wants
  readers-only builds moves the shared state into a `Model`; frus does not infer which part of
  `view` read which field of the state.
- **No device run.** Everything is checked through the shell's frame without a window
  (`Driver`/`Tester`); the feature has no platform code.
