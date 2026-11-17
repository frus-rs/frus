# Milestone 817 — A wrapper's box, once

## Objective

Milestone 812 left two known faults, each worked around inside the Apple-style dialogs:

- **A padding straight under a `ThemeBuilder` was applied twice.** The builder takes its
  child's style as its own, so that putting one around a widget does not move it. It took the
  whole style, padding and margin included, and the child then applied both again inside it:
  an 8-pixel padding came out at 16. The action sheet held its padding in a plain column to
  avoid it.
- **`Expanded` replaced its child's least size with zero on both axes.** A 45-pixel button in
  an expanded slot of a row laid out at its text's height. The alert held each side-by-side
  button in a plain column to keep its 45 pixels.

This milestone fixes both at their source, applies the same fix to every other wrapper that
takes its child's box, and removes the two workarounds.

## What changed

### A wrapper that nests its child takes the box, not the inside of it

A new private module, `nested.rs`, states the split a box already makes:

- **Around** the box (size, flex, margin) belongs to the wrapper, since the wrapper is what its
  parent places.
- **Inside** the box (padding, and how children are laid) belongs to the child, since the child
  is what holds them.

`nested_box(style)` is the child's style less its padding. `Unmargined` holds the child in every
respect but its margin, which the wrapper has already left; `Unmargined::split` hands back both
the child itself (to ask for its style) and the node to hand out as the wrapper's child.

The wrappers that are a layout node of their own and take their child's box now all go through
it:

- `ThemeBuilder`;
- `Tooltip`, `RichTooltip`, `HoverCard`, `ContextMenu`, `CommandPalette` (an anchor with an
  overlay);
- `ScrollOverlay`.

`Tooltip` already dropped the padding (milestone 632) but still left the margin twice.

Seven private one-line adapters (`BoxedTitle` in the app bar, `BoxedChild` in the carousel,
`Boxed` in the date picker dialog and the glass panel, `BoxedItem` in the expanding rail,
`BoxedCard` in the KPI card, `BoxedPart` in the sidebar) did the same thing by hand: they took a
boxed child's whole style and nested the child under it. A `Box<dyn Widget>` is a widget in its
own right, so each was removed and the boxed child is passed directly.

### A flexible child's least size along the line, and only there

`flex_item` (the style `Expanded` and `Flexible` give their child) now lifts only the
**automatic** floor. A least size the child asked for is kept.

Along the parent's line, a tight share must still overrule it: an `Expanded` is exactly its
share. Only the parent knows which axis is its line, so the parent decides. A new structural
hook, `Widget::fills_flex_share()`, answers `true` for a tight `Expanded` or `Flexible`. A row or
column (any box that is not a stack or a grid) asks each child and, for those that say yes,
clears the least size on the line's axis only, through the new
`frus_layout::Layout::clear_min(node, horizontal)`. Across the line the child's floor stands.

The hook is forwarded like the other structural hooks: every transparent wrapper forwards it
(`forward_flex_hook!` in `forward_transparent!`, with a new `flex` arm for `Expanded` and
`Flexible`, which state it themselves), and so do `Responsive`, `ThemeBuilder` and
`Box<dyn Widget>`.

### The workarounds are gone

- The alert's side-by-side buttons sit straight in their `Expanded` slots.
- The action sheet's edge padding sits straight under the builder.

## Decisions

- **Split the style rather than give the wrapper an empty one.** A wrapper with a default style
  would no longer sit where its child would have sat: a fixed width, a flex share or an
  alignment on the child would stop working the moment it was wrapped. Taking everything but
  the inside of the box keeps the transparency the builder was made for.
- **The margin goes to the wrapper, not the child.** The wrapper is what its parent places, so
  the space around it has to be the wrapper's for the parent to see it; the child leaves none.
- **The parent clears the floor, not the child.** The child's style has no way to say "along my
  parent's line"; asking the child whether it fills a share and letting the parent pick the
  axis keeps `Expanded` correct in rows and columns alike.
- **Loose children keep their floor on both axes.** A `Flexible` with a loose fit, or a loose
  `Expanded`, is not handed an exact share, so a least size it asked for is still a floor.

## How tested

- `themebuilder.rs`: `a_padding_under_the_builder_pads_once` and
  `a_margin_under_the_builder_is_left_once` lay out a column with and without the builder
  around a padded or margined box and compare where both boxes land.
- `tooltip.rs`: `a_tooltip_leaves_its_child_s_margin_and_padding_once`, the same for a tooltip
  around a box with both.
- `expanded.rs`: `it_keeps_the_child_s_least_size_across_the_line` (a row and a column, tight
  and loose), `along_the_line_a_tight_share_overrules_the_child_s_least_size`,
  `along_the_line_a_loose_child_keeps_its_least_size`.
- The existing Apple-style dialog tests keep the alert's 45-pixel buttons and the sheet's
  8-pixel edges without their workarounds.
- The whole workspace's tests, clippy on the host, Android and the web, and the goldens.
