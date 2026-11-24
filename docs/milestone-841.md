# Milestone 841 — Text in SVG pictures and Lottie animations

## Objective

#303 (SVG pictures) and #304 (Lottie animations) had one large piece left: words. An SVG with
a `<text>` element showed without it — the parser frus uses drops text unless it lays it out
itself, with a font stack of its own that frus does not carry, so not even
`Svg::unsupported` said so. A Lottie text layer (type 5) was removed before the animation was
read and named `"text"`. Labels on illustrations, numbers in charts exported as SVG, the
"Done!" of a success animation: all missing.

## What was done

- **`frus_text::outline_text` and `outline_paragraph`** (new, with `OutlineStyle` and
  `OutlinedLine`): words shaped by the same font system as every `Text` — the fonts the
  application has, its fallbacks, its weights — and each glyph turned into the path its font
  draws it with, unhinted, on a baseline at `y = 0`. A `font-family` list is read as CSS reads
  one: the first loaded family draws, `serif`, `sans-serif` and `monospace` are the generic
  families, and a list naming nothing loaded draws the default family and says so once on the
  console. A family found is interned; one not found is not, so a picture from the network
  does not grow the registry. `outline_paragraph` breaks at line breaks and wraps to a width.
- **SVG** (`frus-svg/src/text.rs`): text is laid out **before** the parser sees the file. Each
  `<text>` is replaced by what the parser understands — groups carrying the element's and its
  `<tspan>`s' attributes (paint, transform, opacity, class, style), each holding a placeholder
  rectangle with a reserved id. The parser then resolves for a placeholder everything it
  resolves for a shape: fill and stroke (gradients, inheritance, style sheets), the transform
  through any groups and `<use>`, its place among the other shapes, clips and masks. Where
  the picture becomes shapes, a placeholder is swapped for the glyphs; a stroke widens them
  like any stroke; text inside a `<clipPath>` clips to its glyphs. Every placeholder covers
  the whole text element, so an `objectBoundingBox` gradient or filter region spans the text.
  - Laid out: `x`, `y`, `dx`, `dy` (single values and per-character lists, a `<tspan>`'s over
    its parent's), `<tspan>` and `<a>`, `text-anchor` per chunk, `font-family`, `font-size`
    (lengths, `em`, percentages, keywords; 16 when none, as browsers), `font-weight` (numbers,
    `bold`, `bolder`, `lighter`), `font-style`, `letter-spacing`, `xml:space`; read from
    attributes, `style` attributes and `<style>` sheets.
  - Named by `unsupported`: `"text on a path"`, `"rotated glyphs"`, `"text length"`,
    `"text decoration"`, `"vertical text"` — the text is drawn plainly along its line.
- **Lottie** (`frus-lottie/src/text.rs`): a text layer becomes a **shape layer** with
  everything else it had — transform, parent, in and out points, masks, mattes, blend — and
  its words as Lottie paths, so velato draws them like any shape. Read from a document: the
  text (lines at `\r`, `\n` or ETX), the font (`f`, looked up in `fonts.list` for its family,
  style and weight), size, justification (left, right, centre about the layer's origin, or
  within a paragraph box), line height, tracking, baseline shift, fill, stroke (over or under
  the fill), and a paragraph box (`sz`, `ps`) in which lines wrap, the first baseline an
  ascent (the font list's, else 0.75 em) below its top. A **keyframed document** is one group
  per document, each shown from its keyframe to the next by a held opacity: the text changes
  on its frame, never in between. A layer with text animators is drawn without them and
  named `"text animator"`; text on a path is named `"text on a path"`.
- `frus-test` builds resvg with its text layout, to compare against.

## Why outlines, and why before the parser

Drawing artwork text as glyph outlines is what lets it be turned, skewed, filled with a
gradient, stroked and clipped exactly as the artwork says, at any size; the text renderer's
glyph atlas draws upright text in a flat colour. The outlines come from frus's own fonts, so
a picture's words look like the application's, and no second font stack is shipped.

Laying text out before the parser, rather than walking the XML separately, keeps the parser
responsible for what it does well (styles, `<use>`, transforms, paint servers, document
order). The alternative — the parser's own text feature — would bring a second shaper and
font database (second copies, at other versions, of crates frus already has) into every
application with pictures, and would lay text out with fonts the application does not have.

## Verified

- `frus-text`: an `H` stands on its baseline with a cap height above it; twice the size is the
  same outline twice as large; bold is wider, spacing adds its room, an unknown family before
  `monospace` draws monospace; a paragraph breaks and wraps.
- `frus-svg`: glyphs on the baseline at `x`, in the fill (on master the parser drops the
  text, so this finds no shape); `start`, `middle` and `end` anchors; a `<tspan>` with its own
  paint, size, `dx`/`dy` and an absolute `x`; position lists; white space collapsed and
  preserved; bold, a style-sheet class, an inherited family list; a transformed text with an
  `objectBoundingBox` gradient spanning it, a clip path made of text, document order; a
  stroked text widened, an invisible one not drawn; the unsupported features named.
- `frus-lottie` (`tests/text.rs`): a text layer drawn at its position in its size and colour
  (fails on master: nothing is drawn and `"text"` is named); left, right and centre
  justification and a second line a line height down; bold from the font list, tracking, a
  stroke over the fill; a keyframed document switching on its frame; a paragraph box
  wrapping; animators named. All six run against master's `frus-lottie` fail.
- On the GPU (`frus-test/tests/svg.rs`, `text`): a picture with anchored, bold, styled,
  `<tspan>`-offset, gradient-filled and rotated text, drawn by frus and by resvg laying out
  the same DejaVu Sans files, agree on all but at most 2 % of pixels (the laptop's GPU).
- `cargo clippy --workspace --all-targets -D warnings` clean. The goldens were not run: no
  renderer code changed, and `frus-text`'s existing paths are untouched (the outlines are a
  new module).

## Not done

- SVG: `<textPath>`, per-glyph `rotate`, `textLength`/`lengthAdjust`, `text-decoration`,
  vertical writing modes, `baseline-shift`, `dominant-baseline`/`alignment-baseline` (all text
  sits on the alphabetic baseline), `direction="rtl"` with `text-anchor` (an anchor is applied
  as if the chunk ran left to right), the `font` shorthand, `font-variant` and
  `font-stretch`. Style sheets are matched against the original elements for fonts only; a
  sheet rule that selects `text` or `tspan` by element name gives its paint to nothing,
  because those elements become groups (class and id selectors work). Percentages in
  positions are ignored. `<text>` inside an SVG embedded as an image is not drawn.
- Lottie: text animators (range selectors, per-character transforms and colours), text on a
  path, the glyph shapes a file may embed in `chars` (the font list is used instead, so a
  file drawn with a font the application does not have draws in the default family), caps
  (`ca`), and the justify-last-line modes beyond treating them as their left, right or
  centre base.
- Text in pictures is shaped when the picture is parsed: a font registered afterwards is not
  used until the picture is parsed again.
- Not checked on the test phone or in a browser.
