# Milestone 836 — The home page's frame on a slow phone's CPU

## Objective

After milestone 835 the demo's home page, scrolled on the test phone (Mali-G51, Cortex-A53
cores, Android 10), drew a frame about every 50 ms: about 23 ms of GPU and 18 to 30 ms of
layout and paint on the CPU, in frames that rebuild nothing. This milestone finds where those
CPU milliseconds go and cuts them.

## Where the time went

A test in the demo (`home_frame_cost`, ignored, run in release) builds the frame the shell
builds while the list scrolls: the retained tree, laid out and painted at a new scroll offset
each time, with no rebuild. On the desktop it cost **16 to 17 ms a frame**, far too much for a
page of about a hundred widgets. `perf` (under WSL) put 30 % of the time in taffy's flexbox
layout alone and most of the rest under it, and the relayout cache's counters said why: every
frame recomputed the page's layout roots from scratch.

The page holds one `LayoutBuilder`, the summary line under the task list (the count of tasks
and how many are left). The relayout cache fingerprints a root's styles and structure and reuses its geometry
when the fingerprint has not moved, but a `LayoutBuilder`'s closure cannot be fingerprinted
by what it builds, so it marked the root holding it *volatile*: never stored, laid out again
every frame. The fingerprint walk goes into a viewport that is sized by its content, so the
mark climbed to every root above the summary. Two roots were recomputed each frame:

| Root | Rectangles | Time per frame (desktop) |
|---|---|---|
| The page around the scroll view | 2 | 9 to 16 ms |
| The scroll view's content | 37 | 2.6 to 5.6 ms |

The outer root has two rectangles and costs the most because its one child is a measured leaf:
each time taffy asks it a question, it lays the whole page out again in a tree of its own, and
the measured leaves inside it (the summary, the two horizontal strips) do the same in turn.

A smaller thing showed up beside it: one root was laid out under two constraints in the same
frame (asked how wide it would be with its width free, then given 109 pixels). The cache kept
one entry per root, so each question evicted the other's answer and both were computed every
frame.

## The change

### A `LayoutBuilder` is known by its identity

`Widget::layout_builder_identity` is new: a number that stays the same for as long as the
factory does and that no other factory has carried before. Each `LayoutBuilder` takes one from a
counter when it is made. The fingerprint hashes that number, with the surface description the
closure may read (`MediaQuery::measure_hash`), instead of marking the root volatile.

So a `LayoutBuilder` kept in the tree from one frame to the next, under the same constraints and
on the same surface, is not asked again, and its geometry is reused. One made again (its
component rebuilt, or the view rebuilt) is a new number, and is asked again. That is the usual
contract of a layout callback: it runs when its box changes or when the widget is built again.
The closure must build from the box it is given and what it captured, which the widget's docs
already asked of it; what changes what it builds has to rebuild the widget that holds it. The
`LayoutBuilder` module docs say so now.

A counter, not the closure's address: a factory built inside another one's closure lives in the
frame's arena, and the next frame may put a different factory at the same address. A counter is
never reused.

A factory with no identity keeps the old answer. The other widgets that answer
`layout_builder` (`GridView`'s built cells, `OverflowBar`, the resizable group) build their
closures from their own state, so they stay volatile. The new method is forwarded by `Box`,
the transparent wrappers' macro and `Responsive`, like `layout_builder` itself.

### Two entries per root

The relayout cache now keeps up to two entries per root, the most recent first. A second entry
is kept only while it describes the same tree (the same fingerprint) under other constraints;
a changed tree drops it.

## Measured

### Desktop

`home_frame_cost`, release (no LTO, for symbols), 400 frames, WSL on the development machine:

| | Before | After |
|---|---|---|
| Layout + paint, per frame | 16.2 to 17.2 ms | 2.3 to 2.7 ms |
| Layout roots recomputed per frame | 4 | 0 |

### Phone

Not measured. Release APKs of the base (master with milestone 835's shader) and of this change
were built for the interleaved comparison of milestone 835, but the phone was locked when the
run started and later disconnected; this milestone does not unlock it. See **Not done**.

## How it was tested

- `relayout::tests::a_retained_layout_builder_is_laid_out_once`: a column holding a
  `LayoutBuilder` laid out twice through the cache is a miss and then a hit, and the closure is
  not run on the second; new constraints, a new surface, and a builder made again (reading
  alike) are each a miss. It replaces `a_layout_builder_still_poisons_its_root`, which asserted
  the old behaviour.
- `relayout::tests::a_factory_without_an_identity_still_poisons_its_root`: a factory with no
  identity is still recomputed every frame.
- `relayout::tests::two_constraints_a_frame_are_both_kept`: a root laid out under two
  constraints each frame is two hits from the second frame on; a changed tree misses under
  both and does not bring the old tree's answer back.
- `tests::scrolling_the_home_page_lays_nothing_out_again` (frus-demo): the demo's own home
  page, settled with six tasks, scrolled over six frames with no rebuild, recomputes no layout
  root on the last one. **Before the change it fails** (four roots a frame).
- `tests::home_frame_cost` (frus-demo, ignored): the timing above.
- **The tests fail without the change.** With the identity ignored (every `LayoutBuilder`
  volatile again), `a_retained_layout_builder_is_laid_out_once` fails, and
  `scrolling_the_home_page_lays_nothing_out_again` fails with "a scrolled frame laid 2 roots
  out again". With one entry per root again, `two_constraints_a_frame_are_both_kept` fails.
- `cargo clippy --workspace --all-targets -D warnings` (and for `aarch64-linux-android` on
  frus-widgets), the frus-widgets, frus-layout and frus-demo tests, the frus-widgets doctests
  and its docs with `-D warnings` pass.
- The goldens (frus-test, all targets, under WSL, `CARGO_INCREMENTAL=0`) pass unchanged: 106
  and 50 images among the rest. A reused layout is the same geometry the computation gives, so
  no picture moves.

## Not done

- **Not measured on the phone.** The phone was locked, then disconnected, while this milestone
  ran. The desktop harness shows the frame's layout and paint falling from 16 to 17 ms to 2.3
  to 2.7 ms, and the cache recomputing nothing while the list scrolls; on the phone the same
  frame took 18 to 30 ms, so a cut of the same order is expected but has not been seen. The
  APKs and the measuring script are ready for the interleaved comparison.
- **What is left of the frame on the CPU.** On the desktop the remaining 2.3 ms are spread
  out: the fingerprint walk (SipHash, about 6 %), copies of the 11.5 KB theme made by every
  button and every themed widget in each walk (`theme_override` returns a whole `Theme`), the
  paint walk itself, and allocation. None of them dominates the way the layout did.
- **A miss is still expensive.** When a root does have to be laid out again (the first frame,
  a rebuild, a new window size), a measured leaf still lays its subtree out in a fresh tree of
  its own for every question taffy asks, and nested measured leaves multiply that. Caching
  measurements within a frame would cut it; it is not done here.
- **Other layout builders.** `GridView`'s built cells, `OverflowBar` and the resizable group
  still mark their roots volatile.
