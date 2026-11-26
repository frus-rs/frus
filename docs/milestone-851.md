# Milestone 851 — A page gets the whole screen, shells nest, table labels wrap

Fixes #516. Answers L-045 (nested shells) and L-047 (a table cell's label on one line), and
clears the Android lint run of `clippy::missing_const_for_thread_local`.

## Objective

Four things that were wrong on `master`:

1. **The demo's Apple-style screen showed its two bars and nothing between them** (#516). The
   issue was found on the web, but the screen's scrolling body had no height on every
   platform.
2. **A router shell inside another shell's branch panicked** when the router was made (L-045):
   tabs inside a tab, each with its own stack, could not be written.
3. **A table cell painted its label on one line** (L-047): a table narrower than its labels
   ran past its box, and a virtualised table drew a long label over the next column.
4. **`cargo clippy --target aarch64-linux-android --no-deps -- -D warnings` failed** on
   `missing_const_for_thread_local`, so nobody linted the Android-only code.

## 1. A page is handed the whole navigator (#516)

**Root cause.** A navigator laid each page out as a root asked how big it wanted to be
(`Constraints::definite`): a page whose root fills (a `Scaffold`) took the screen, and any
other root hugged its content. The Apple-style screen's root is a coloured `Container` around
an alert, an action sheet and a column — the navigation bar, a `SingleChildScrollView` with
`flex(1.0)`, the tab bar. The container hugged the column, the dialogs' `height: 100%` resolved
against that, and the column had no rest to give its body: a node dump of the screen showed
the container 94 px tall (44 + 50, the two bars) and the scroll view 0 px tall, at 400×800 and
1280×720, under Android, iOS, macOS, Windows and Linux themes alike. The web only made it
visible first.

**The fix** (`ui.rs`, `render_screen`): a navigator's page is laid out *handed* its box
(`Constraints::filled`), as a route hands its page the screen. An `Auto` dimension of the
page's root becomes the navigator's size; a size the root gives itself still stands. Pages
whose root already filled (every `Scaffold`) are unchanged.

## 2. A shell inside another shell's branch (L-045)

A route and a page now carry the **path of shells** they are in, the outermost first
(`ShellPath`, an `Rc<[(shell, branch)]>`), instead of one optional pair. A shell's number is
taken before its branches are built, so an inner shell gets its own.

- **Building the stack.** A `go` makes each page with its route's path. A location where a
  branch was left finds that branch's saved stack at the *outermost* level whose saved stack
  ends there, so an inner shell comes back with its outer tab. A `push` walks the levels: as
  deep as the shells on show are the page's own, switching an inner branch first when the page
  belongs to another one; it stops where the page's shell is not on show, and the page covers
  what is below it there.
- **Keeping the branches.** Every level's branch on show is kept in step with the stack. An
  inner shell whose outer tab is not on show is kept with that tab, so its own tabs are as they
  were when the outer tab is chosen again.
- **Back** goes from the inside out: the inner branch's own pages, the inner shell's
  `ShellBack`, then the outer shell's, then the pages beneath. `GoRouter::go_branch` switches
  the innermost shell's tabs; a frame's `NavigationShell` switches its own.
- **A branch whose first route is a shell** opens where that shell's first branch does.
- **Drawing.** The navigators are built by one recursive `Assembly`: a navigator `depth` shells
  deep shows each page that is in no deeper shell and each deeper shell once, as a frame
  around a navigator of its own. A motion plays in the navigator of the innermost shell both
  pages share a branch of; between two branches of a shell, in that shell's navigator, if it
  has a branch transition. A page leaving with its shell is drawn as the shells were.

Restoration is unchanged: it replays the stack on show, so the branches not on show restart
from their first page, at every level (the rest of L-045).

## 3. A table cell's label wraps (L-047)

- **A text cell is measured** (`Cell::measure`): as tall as its label laid out at the column's
  width, less the padding and a heading's icon and sort arrow, with 8 px above and below. The
  paint wraps the label at the same width and centres the block.
- **A flexible column** starts from a basis shared by every flexible column — the widest
  label of them all — grows from it in equal shares, and may shrink down to its own widest
  *word* (its floor was its widest label, with a zero basis). A table wider than the basis
  times the columns gives equal shares, as before; a narrower one shrinks them alike and wraps
  the labels inside its box. Where the width is unbounded (a row beside other children, a
  horizontal scroll) a cell asks for that basis, the same in every row, so the rows agree on
  the edges and no label wraps: the layout sums what the cells ask for, and a cell that asked
  for its own label made the table only as wide as its labels, then shrank the columns alike.
  A heading holding an action widget still paints its label on one line, and its column's
  floor stays the whole label.
- **Row heights**, as the leading toolkit's data table has them: `data_row_min_height` (34
  unset), `data_row_max_height` (no limit unset; past it a label is cut at the row's edge) and
  `heading_row_height` (34 unset), on `Table`, on `DataTable`, and in `TableTheme`, which is
  now exported. A table's own word reaches its cells through the theme, like its text styles;
  `DataTable` forwards its table's `theme_override` when it is the bare table.
- **Where something else sets the rows' heights** — a virtualised table's list, frozen columns'
  separate blocks, a resizable grid's handle layer — a label stays on one line and is clipped
  to its cell instead of running into the next one.

## 4. `missing_const_for_thread_local` on Android

The lint was reproduced with the clippy of Rust 1.96 (the machine's default toolchain):
`--target aarch64-linux-android` reports it on `text_style.rs:776` and on eleven blocks in
`frus-widgets` (`component.rs`, `dismiss.rs`, `animationhandle.rs`, `locale.rs`, `media.rs`,
`restoration.rs`, `scaffoldinfo.rs`, `scrollbehavior.rs`, `textservices.rs`, `host.rs`,
`messenger.rs`). Every one of those blocks already declares its initializers `const { … }`
where they can be (the two that do not, `host.rs`'s `SETTINGS` and `messenger.rs`'s
`MESSENGER`, call a `Default` that is not `const`): the report is about the expansion of
`thread_local!` on a target whose thread locals go through the platform's keys, not about the
code. The clippy of Rust 1.100, the stable line CI follows, reports nothing:
`cargo +1.100.0 clippy --workspace --target aarch64-linux-android --no-deps -- -D warnings`
passes on this branch, and CI's own Android lint job passed on `master` (`ec11236e`). No code
was changed for it: a machine whose default toolchain is older sees reports CI does not.

## How it was tested

- **#516**: `the_apple_style_screen_s_body_takes_the_room` (frus-demo) opens the screen under
  three platforms at two sizes and checks the scrolling body's viewport. Before the fix the
  same probe printed a viewport 0 px tall in all fifteen cases. `a_page_is_handed_the_whole_navigator`
  (navigator.rs) is the same shape at the widget level: 44 + body + 50 in 800, body 706.
- **L-045**: six tests in `router.rs` (`nested_shell_tests`): the levels a location puts a page
  in, each level keeping its stacks and the page's state across outer tab switches, the
  router's `go_branch` switching the innermost shell, Back from the inside out, a route outside
  both shells covering them, and a build with both frames drawn, three navigators, and a push
  inside the inner tab played by the inner navigator alone. The existing shell tests pass
  unchanged.
- **L-047**: five tests in `table.rs`: a long label wraps inside its fixed column and its row
  grows; a flexible table capped at 240 px wraps both columns inside the box; least and most
  row heights from the table and from the theme; a `DataTable` wraps and hands its row height
  on; a virtualised table clips a long label at its cell. The existing column tests (shares,
  unbounded rows, horizontal scroll, sort arrow room, headings with actions) pass unchanged.

## Not done

- **No release build of the web demo was made.** The browser check ran on a `web-release`
  build with link-time optimisation off and `opt-level = 1`, to fit the time a build may take
  here; the layout code is the same.
- **Container does not hand its constraints down.** A page whose root is a coloured
  `Container` around a `Column` without `flex` still hugs the column: the leading toolkit's
  container passes a tight box on to its child, this one only fills when it has no child or
  aligns it. The Apple-style screen fills because its dialogs are 100 % tall; the general case
  is left for its own step.
- **Process restoration of nested shells**: the branches not on show restart from their first
  page, at every level (L-045 stays partial).
- **Virtualised tables, frozen columns and resizable grids do not wrap** their labels; they
  clip them to the cell (L-047 stays partial).
- **Android**: compiled and linted only; no phone or emulator was used.
- **Goldens were not run**: the table cells and the navigator's page sizing changed, and no
  golden scene was regenerated or compared.
