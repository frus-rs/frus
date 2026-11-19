# Milestone 822 — A component's own state builds that component, not the tree

## Objective

Issue #53: every state change built the whole tree again. A counter ticking in one leaf of a
screen of forty rows called the application's `view`, and so built the screen, its forty rows
and the leaf, to change one number. The tree the last frame laid out was already kept between
frames (a frame that only scrolls, hovers or animates builds nothing); what was missing was a
way to build *part* of it.

From this milestone, a frame whose only reason to build is a component's own state no longer
calls `view`: it walks the kept tree and builds again, where they stand, the components that
were marked. Everything else is kept as it was.

## Design

- **Marking.** Each component's hooks carry a mark. `UseState::set` / `update` and
  `StateHandle::set_state` set the mark of the component that **owns** the state (not of the
  one whose handler made the change) and ask for a frame. Everything else that asks for a
  rebuild (`request_rebuild`: a controller, a setting, a router, an effect, a preference) still
  asks for the whole tree. The new `take_rebuild()` tells the shell which, as a `Rebuild`:
  `Nothing`, `Components` or `Tree`. `take_rebuild_request()` and `rebuild_requested()` are
  unchanged.
- **Building in place.** `Component` keeps its build (`Fn`, no longer `FnOnce`) and what it
  built. The walk that builds components reaches every component of the kept tree. One built
  in an earlier rebuild is either **kept**, or **built again** when marked. A kept component
  has its state entry touched so the sweep keeps it, and the timers and back blocks of its
  last build are asked for again. A component built again runs its build, and what that
  returns replaces its child. What it built is new, so everything under it is built too.
- **Inherited data.** Theme, surface (`MediaQuery`), locale, platform settings, text scale,
  title bar and insets already set the shell's `build_dirty`, which still means the whole tree
  is built from `view`. A local scope (`Themed`, `MediaScope::tweak`) is part of a component's
  output, so it changes only when that component's state changes, which builds that component
  and everything under it. Nothing below a kept component can see a scope that changed.
- **Configuration.** A parent built again makes new configurations for its children. Widgets
  are not `PartialEq`, so the framework cannot compare them, and the children are built too: a
  stateful child is told with `did_update_widget`. A component built again for its own state's
  sake has the same configuration, and `did_update_widget` is not called.
- **Keys and transparent wrappers** are unchanged. A component's identity is still its place
  (or its key) and its depth in a component chain. `Keyed`, `Responsive` and the rest forward
  `expand`, so a marked component inside one is reached and built.
- **Elm-style applications** (`Application::view`) are unchanged: a message still builds the
  whole tree, because their `view` reads what `update` changed. The new
  `Application::messages_rebuild_view` (default `true`) says so. `FrusApp` answers `false`, and
  a handler of it that marks nothing still builds the whole tree, as before.
- **Hot reload.** A patch arrives as a task arrival, which builds the whole tree, every
  component through `HotFn::current`. A component built again later still goes through
  `HotFn::current`.

### Replacing a child through `&self`

Every walk reads the tree through shared references, and `children()` hands out slices of what
a component built, so the slot is an `UnsafeCell`. It is written only by the component's own
`expand`, at the first visit of a rebuild, before the walk reads anything below it. The child
it replaces is **retired**, not dropped, until a later rebuild reaches the component, so a
reference into the old child taken earlier in the same walk stays valid. A frame's walks borrow
the tree, and the next rebuild needs it again, so no walk of the earlier one can be under way.

## Alternatives rejected

- **Caching by key and version** (`Keyed` plus a version the application bumps). This asks
  every application to say by hand what changed, and a forgotten bump is a stale UI with no
  error.
- **Comparing configurations** (widgets `PartialEq`). Two hundred widget types hold closures
  and boxed children, which cannot be compared.
- **Rebuilding the path from the root down to the marked component.** With no comparison, the
  root built again builds everything, which is the problem itself.
- **Recording what each component read** (fine-grained dependency tracking of shared data).
  This needs every read of every shared value to go through the framework. The hooks already
  say who owns a state, which is the dependency that matters here.
- **Extending the layout and paint caches** instead of skipping the build. They already reuse
  what they can, and the build has to run before either cache is asked anything.

## Numbers

`rebuild_cost_of_one_leaf_tick` (`crates/frus-shell/src/rebuild_tests.rs`, ignored by default,
run by hand: `cargo test -p frus-shell --release rebuild_cost -- --ignored --nocapture`). A
screen of forty rows with a counter ticking in one leaf, 300 ticks through the shell's own
frame, Windows release build, medians:

| | build | layout | built per tick |
|---|---|---|---|
| before (master) | 0.261 ms | 8.554 ms | 1 screen, 40 rows, the leaf |
| after | 0.100 ms | 7.825 ms | the leaf |

The build is 2.6 times cheaper here, and it no longer grows with the screen: the forty rows
and the screen are not built at all. The layout still walks the whole tree. It is now the cost
that dominates a tick, and it is where the next gain lies.

## Tests

- `frus-widgets` (`component.rs`): a leaf's own state builds the leaf alone; a parent's state
  builds its subtree and keeps the states below; `set_state` builds the stateful widget alone;
  a whole build after marked ones builds everything and keeps the states; a keyed leaf is
  built again through its key; a request says what is to be built.
- `frus-shell` (`rebuild_tests.rs`, through the shell's frame and `Driver`): a leaf tick builds
  the leaf alone, and so does a tap that changes a leaf; a handler that marks nothing builds the
  tree; a theme change and a new surface build their dependents and keep the state; a patch
  builds the whole tree; plus the measurement above. `Driver` gained `resize`, `arrive` and
  `costs` for them.
- The thread-local guard (`frus-widgets/tests/thread_locals.rs`) skipped only what follows a
  `#[cfg(test)]` line in the same file, so the thread-locals of `rebuild_tests.rs` (a test module
  in a file of its own) were reported as shipped code. It now skips `tests.rs` and `*_tests.rs`,
  which are all declared under `#[cfg(test)]`.

## Checks

- clippy on the host (`--workspace --all-targets`), and with `--no-deps` on `frus-shell`,
  `frus-widgets` and `frus-gpu` for `aarch64-linux-android` and `wasm32-unknown-unknown`
  (`-D warnings`).
- `cargo test --workspace --exclude frus-gpu --exclude frus-test` and the golden suite under
  WSL.
- The 8 projects in `frus-examples` (tests compiled) and the gallery for
  `wasm32-unknown-unknown`.
