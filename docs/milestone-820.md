# Milestone 820 — The README shows an application, and a screenshot shows only it

## Objective

The README opened on a GIF of the demo application at a desktop size, and the first thing a
visitor learnt about frus was how big the window was. Two changes:

- **The README leads with a real application on a phone**: two lines on what frus is, Forge (the
  project-management example in `frus-examples/forge`) on a simulated phone, then the quick
  start (`frus create`, `frus run`), the features, and the links (the guide,
  `LIMITATIONS.md`, the examples, https://frus.dev). `README.fr.md` follows the same order.
- **A screenshot shows the application, not the developer's tools.** Pictures for a page or a
  store listing are taken from a running debug build, and whatever the developer had turned on
  (the performance panel, the inspector, debug paint, the repaint rainbow) was in them.

## What was there

There is no always-on debug banner in frus: a debug build draws nothing over the application
until the developer asks. What can be over it is what `frus run`'s keys and DevTools turn on:

| layer | turned on by | painted in |
|---|---|---|
| performance panel | `P`, DevTools | `App::performance_over` (`frus-shell/src/app.rs`) |
| inspector card and outlines | `i` | `frus_widgets::paint_inspector_overlay` |
| debug paint (outlines only) | `p` | the same |
| the widget DevTools selected | DevTools | `frus_widgets::paint_highlight` |
| repaint rainbow | DevTools | `app/devtools.rs::paint_rainbow` |

All are painted into the frame before it is drawn, and the screenshot was drawn from that same
frame: `s` (`write_screenshot`) and DevTools' `screenshot` (`answer_screenshots`, which the
`frus mcp` tool `screenshot` uses) both photographed every layer on screen.

**Release builds draw none of them.** The inspector, the rainbow and DevTools are compiled only
under `debug_assertions`; the simulated device too (`Simulated::from_env` refuses in release);
the performance panel and debug paint appear only when `frus run` sends the command for them.
Nothing in a release build paints over the application unasked.

## What changed

### The clean frame is kept when a screenshot is due

When a screenshot is due this frame (`App::screenshot_due`: `s` was pressed, or a DevTools
`screenshot` waits), the frame is composed twice from the point where the first debug layer
would go: the frame as shown, and the frame without the layers. The steps that are not debug
layers (the see-through backdrop of milestone 643, the simulated device's outline, status bar
and keyboard of milestone 721) are applied to both. The copy is only made on a frame that is
photographed and has a layer to leave out; every other frame pays a boolean.

`photographed(shown, clean, overlays)` picks the picture: the clean frame unless `overlays` is
asked, and the frame as shown when no clean copy was needed.

### The switch

- **`s` in `frus run`** photographs the clean frame, always. That is what a picture is taken for.
- **DevTools `screenshot`** takes `{"overlays": true}` to keep the layers, for a picture of the
  inspector or the panel itself. Left out, the picture is clean. It also takes `{"scale": n}`
  (0.25 to 4): the frame drawn `n` times larger, for a sharper picture than the monitor the
  window is on. Requests asking the same are drawn once.
- **`frus mcp`'s `screenshot` tool** has the two arguments, `overlays` and `scale`, and passes
  them on.

The simulated device is not a debug layer: a picture of an application on a simulated phone
keeps the phone. A picture without it is the application started without `-d <device>`.

### The README

The top is two lines (unchanged), the badges (unchanged), a line of links, and four pictures of
Forge on the simulated Android phone, side by side. Then the quick start (`cargo install
frus-cli`, `frus doctor`, `frus create`, `frus run`) and the samples of this repository, then
**Features** (what frus is, components, why, the demo's gallery, the `frus` tool, Android, the
web, simulated devices), then **Learn more** (the guide, `LIMITATIONS.md`, the examples, the
website, the documentation list), then architecture, status, where to start, contributing and
the licence, as before. The French README has the same structure and no em dash.

`LIMITATIONS.md` is written by milestone 818, in parallel: the link resolves once that merges.

### The pictures

`docs/media/forge-*.png`: Forge built in debug from `frus-examples/forge/frontend/forge`,
started with `FRUS_DEVICE=android-phone` and `FRUS_RESTORE_LOCATION=<route>` for each screen,
photographed through the DevTools channel (`devtools {"method":"screenshot","params":{"scale":
2}}` on the socket `FRUS_DEV` names), then cropped to the phone, the backdrop around it made
transparent, and quantised to keep each file small (10 to 15 KB at 400 pixels wide).

The four screens, in order: the projects (`/projects`), a project's board (`/projects/1`), its
overview (`/projects/1/overview`) and a task's details (`/tasks/125`, saved as
`forge-task.png`). No picture shows a member's full name. The pictures that did were left out:
the dashboard greets the signed-in member, and task 126 and the project's activity name the same
member. The notifications page was dropped too: it always shows "You're all caught up", with no
list.

## Found

- **Forge: the assignee chip stretches.** On `/tasks/128`, a task with no priority and no due
  date, the assignee chip takes the whole width of the row instead of hugging its content.
  Not fixed here.
- **Capturing under Git Bash needs `MSYS_NO_PATHCONV=1`.** Git Bash rewrites an argument that
  starts with `/` into a Windows path, so the route `/tasks/1` reached Forge as
  `C:/Program Files/Git/tasks/1`. Set `MSYS_NO_PATHCONV=1` when you run the capture script, and
  pass Windows paths (`D:/…`) for files.

## How it was tested

- `screenshot_tests::the_performance_panel_is_on_screen_but_not_in_the_picture`
  (`frus-shell/src/app.rs`): the performance panel painted over a page puts its lines in the
  frame shown; the picture taken without `overlays` has exactly the page's primitives and no
  panel text; with `overlays` it has the panel; and, where a GPU is at hand, the picture is the
  page's white in every pixel while the frame shown differs where the panel is.
- `devtools::tests::a_screenshot_leaves_the_debug_layers_out_unless_asked` and
  `a_screenshot_can_be_drawn_larger`: the parameters as the protocol reads them.
- The Forge pictures themselves were taken with the new path, on a real window.

## Not done

- **The web and Android** photograph what is on screen: a page reads its canvas, and on
  Android `frus run` takes the device's own screenshot. Turn the layers off before taking one
  there. Drawing the clean frame offscreen on those platforms would need an offscreen renderer
  they do not have yet.
- **`docs/guide/devtools.md`** (the protocol) does not list `overlays` and `scale` yet: the
  guide is being rewritten by milestone 819 at the same time. The parameters are documented in
  the rustdoc of `devtools.rs`, the MCP tool's schema and this note; the guide should take them.
- The demo's own pictures (`tasks.png`, `charts.png`…) are rendered offscreen by the `shots`
  tool, which never had debug layers; they are unchanged.
