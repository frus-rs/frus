# Milestone 853 — A change lays out the path to it, not the whole tree

## Objective

L-042: a rebuild walked the whole widget tree. Milestone 822 made a component's own state
build that component alone, and measured what was left: a counter ticking in one row of a
screen of forty rows still cost 8 ms of layout, every tick, because the layout of the
scroll view holding the rows was computed from scratch. This milestone measured that cost
again, found where it goes, and makes a change lay out the boxes between it and its layout
root, with every other box answering from what it measured a frame earlier.

## What the frame cost before

Measured through the shell's own frame (`rebuild_cost_of_one_leaf_tick`, a ticker above
forty `ListTile` rows in a `SingleChildScrollView`), Windows laptop, release build without
LTO (see *Numbers*), with timers around each stage of `build_ui`. Per tick:

| stage | ms |
|---|---|
| build (milestone 822: the ticker alone) | 0.05 |
| layout fingerprints (`signature_of`) | 0.4 |
| building the layout trees (`build_layout`) | 0.2 |
| the flexbox computation itself | **3.0** |
| painting and registering (the walk) | 0.8 |

Two thirds of the frame was the flexbox computation, and it was paid three times: the
layout of the scaffold's body measures the scroll view, the scroll view measures its content
(a viewport sized by what it holds, milestone 590) in a tree of its own, and the content is
then laid out as its own root. Each one rebuilt its tree and computed every node.

## Design

### The layout keeps its answers between frames

`frus-layout` now owns its node tree (`nodes.rs`) and runs taffy's flexbox, grid and block
algorithms on it through taffy's low-level traits, instead of using `TaffyTree`. The
algorithms are taffy's, unchanged, and so is taffy's per-node cache: the sizes a node gave
under which constraints, and its last placement.

Owning the tree is what lets that cache outlive the frame:

- `Layout::into_memo` keeps, after a layout, each node's style, content key, children,
  cache and placement (a `Memo`).
- `Layout::reuse(memo, context)` hands it to the next layout of the same root. Before the
  first computation, each node of the new tree is matched against the memo, children first:
  a node is **the same** as a node of the last layout when it has the same identity (the
  widget's `WidgetId`, passed with `set_identity`), the same style, the same content key and
  the same children, all the way down. A node that is the same starts with that node's cache.
- Asked the same question, it answers without being laid out. Asked to place itself the
  way it was placed, it also puts back the placement of every box below it, since the
  algorithms that would have placed them are not run.
- A change deep in a tree therefore lays out the path from it to the root, and each box on
  that path asks its other children what they already know. The rectangles are those of a
  layout from scratch: the reuse rule is taffy's own cache rule, which taffy already applies
  inside one computation, and the rounding to whole pixels runs over the whole tree as before.

A node is the same only if it says what it depends on. A plain box depends on its style. A
measured leaf (a paragraph that wraps, a growing field) depends on what `Widget::measure_key`
says, on the reader's font size and on the surface it is measured under (`MediaQuery`); one
whose widget says nothing is measured again every time, and so is every box above it. The
`context` hashes the font size and the surface at the root: a memo taken under another is not
used.

`Text::measure_key` now includes the letter spacing, which the measurement adds and the key
left out; with answers kept node by node, a spacing changed alone would have kept the old
width.

### Where the memos live

- **Layout roots** (`LayoutCache`, milestone 55): each entry keeps the memo of the layout
  that made it. A root whose fingerprint changed is computed again, as before, but starting
  from that memo.
- **Trees a measurement lays out on its own** (a viewport measuring its content, a box that
  transforms its child's constraints, a stack measuring a layer, a layout builder): their
  memos live in `Runtime::nested_layouts`, keyed by the identity of the subtree and what it
  is laid out for, and are forgotten when a frame does not lay them out. Apart from the
  layout cache, because that one is borrowed while a root is computed, which is when these
  measurements run.
- **Within one layout**, taffy asks a measurement several questions (how wide at most, how
  tall at this width). A measurement used to build its tree again for each question; it now
  builds it once (`NestedTree`), puts the root's style back before each question, and lets
  the tree's own cache answer what was already asked. A layout builder is the exception: it
  builds its child from the size offered, so a new question is a new child.

### What is not reused, and why

- A `LayoutBuilder`'s child, and any measured leaf without a key: their answer depends on
  something the tree cannot see.
- The fingerprint walk (`signature_of`), the building of each layout tree, the matching and
  the rounding are still proportional to the size of the root. They are cheap per node
  (a hash, a comparison), but they are not free: see *Not done*.

## Alternatives rejected

- **Retaining taffy's own `TaffyTree` and editing it in place.** Its measure closures borrow
  the widgets of the frame that built them, so a tree kept across frames would hold
  references into widgets that are gone; and the shell gives no diff of the widget tree to
  edit it with. Rebuilding a cheap tree and carrying the expensive part (the cache) is the
  same saving without either problem.
- **A subtree as a measured leaf of its parent** (a "relayout boundary" in the narrow sense).
  Flexbox gives a child's own style a say in its parent's algorithm (its minimum content
  size, its margins, how it stretches), and a leaf standing for a subtree answers some of
  those differently. Keeping the subtree in the tree and reusing its answers keeps the
  layout bit for bit.
- **Matching by identity alone.** An identity is a position, and the widget at a position
  can change; sameness is checked node by node, the identity only says where to look.
- **Hashing styles to match nodes.** A node is compared with the one node its identity
  names, so equality is one comparison and a hash would add a second walk for nothing.

## Numbers

`cargo test -p frus-shell --release rebuild_cost -- --ignored --nocapture`, Windows laptop,
medians of 300 ticks, run while holding the machine's build lock so that no other build ran.
Release profile without LTO (`CARGO_PROFILE_RELEASE_LTO=false`,
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`): with the shipped profile, the test binary of
`frus-shell` does not compile in the nine minutes a build may take here. Both columns use the
same profile; the "before" binary is `ec11236e` with the same timers.

Forty rows, one ticking leaf (two runs each):

| | layout (`build_ui`) | whole frame | flexbox computation |
|---|---|---|---|
| before | 3.45 / 3.77 ms | 4.13 / 4.89 ms | 2.86 / 3.42 ms |
| after | 1.57 / 1.78 ms | 1.89 / 2.03 ms | 0.46 / 0.49 ms |

Per tick, 9 nodes of the content root are laid out and 240 start from the memo; the nested
measurement of the viewport keeps another 240.

A thousand rows, nothing virtualised, one ticking leaf:

| | layout (`build_ui`) | flexbox computation | fingerprints |
|---|---|---|---|
| before | 391 ms | 99.5 ms | 37.6 ms |
| after | 323 ms | 28.3 ms | 34.5 ms |

At this size the layout is no longer what costs: the walk that paints and registers the rows
takes about 260 ms of the 323, and it grows faster than the number of rows (0.02 ms a row at
forty rows, about 0.26 ms a row at a thousand). That is the next thing to find; it is not
part of this milestone.

The build of milestone 822 is unchanged: no row and no screen is built on a tick.

## Tests

- `frus-layout`: a memo lays out the changed path alone and changes no rectangle (fifty rows,
  one grows a line: the same rectangles as a layout from scratch, and fewer than a tenth of
  the nodes laid out; the same tree again lays out nothing); a memo from another context is
  ignored; an unkeyed measurement is measured again while its sibling is kept; a style
  changed between two passes is laid out again.
- `frus-widgets` (`relayout.rs`): a root whose one row changed lays out its path alone, with
  the rectangles of a layout from scratch, through the real widgets (`Text`, `Flex`,
  `Container`) and the real cache.
- `frus-shell` (`rebuild_tests.rs`, through the shell's frame): a leaf tick lays out the path
  to the leaf (at least 200 nodes kept, at most 30 laid out, the new count shown), next to
  the tests of milestone 822 that the rows and the screen are not built. `Driver` gained
  `layout_nodes`. The measurement gained a thousand-row variant.
- The whole suites of `frus-layout`, `frus-widgets` and `frus-shell`, and the goldens, which
  compare every pixel of the layouts this changes.

## Checks

- clippy `-D warnings` on the host (`--all-targets`), and with `--no-deps` for
  `wasm32-unknown-unknown` and `aarch64-linux-android`, on `frus-layout`, `frus-widgets` and
  `frus-shell`.
- `cargo fmt --all`.

## Not done

- **The fingerprint walk and the building of each layout tree** still visit every widget of
  a root that changed. Skipping them needs to know, without walking it, that a subtree has not
  changed: for a component kept by milestone 822 that is true of its configuration, but not of
  what the runtime adds to it (an animated size, a style in flight, a surface), which the
  fingerprint reads. Written in L-042.
- **The paint walk** visits every widget, and at a thousand rows it grows faster than the rows
  do. The repaint boundaries of milestone 88 reuse a subtree's paint only when no build ran;
  making a kept component's subtree a boundary of its own is the next step.
- **Measured leaves without a key** (a `LayoutBuilder`'s child, a widget that measures itself
  and says nothing) are measured every time, and so is every box above them.
- The numbers are from the laptop; the phone was not used.
