# Milestone 819 — A guide for every subsystem

## Objective

Every subsystem of frus has an accurate page in `docs/guide/`, and `docs/guide/README.md`
indexes them all. The guide had twenty pages, written one milestone at a time around the
feature each milestone added; whole subsystems (accessibility, languages, the screen, the
component model, hot reload, the web, phones) were documented only in rustdoc, in
`getting-started.md` or in milestone notes, and the last six waves of widgets (J809 to J815)
were in no page at all.

## The audit

Subsystems were listed from the crates (`crates/*`) and from what `frus` exports, then each
was matched with the page that covers it.

| Subsystem | Where it lives | Page before | Now |
| --- | --- | --- | --- |
| Component model: `component`, hooks, `StatelessWidget`, `StatefulWidget`, notifiers, keys, restoration | `frus-widgets` (component, notifier, asyncbuild, keyed, restoration) | `getting-started.md` (a summary, not compiled) | **new** `components.md` |
| Layout | `frus-layout`, `frus-widgets` | `layout.md` | fixed (the `Scaffold` and the safe area), points to `screen.md` |
| Screen: `MediaQuery`, size classes, safe area, keyboard, system bars, orientation, simulated devices | `frus-widgets` (media, safearea, responsive, system_ui), `frus-core::device`, `frus-shell::simulated` | one paragraph in `layout.md` | **new** `screen.md` |
| Theme, colours, widget themes, design tokens (`ThemeExtension`), motion | `frus-widgets` (theme, widgettheme, motion) | `style.md`, `animation.md` | covered well; `style.md` gained the Apple-style widgets |
| Text: shaping, fonts as features, your own font, subsetting, selection, right to left, text scale | `frus-text`, `frus-cosmic-text`, `frus-glyphon`, `frus-widgets` (text, richtext, region) | part of `style.md`; fonts only in `getting-started.md` | **new** `text.md`; `style.md` fixed |
| Accessibility: semantics, screen readers per platform, focus order, the reader's settings | `frus-core::semantics`, `frus-widgets` (semantics, focus, barrier), `frus-shell` (a11y, a11y_android, a11y_web, live_region) | none | **new** `accessibility.md` |
| Languages: locale resolution, `Localizations`, seven built-in languages, the application's words | `frus-widgets` (locale, localizations), `frus-l10n` | a section of `style.md` | **new** `localization.md` (the section moved there) |
| Input and forms: fields, controls, `form` rules, autofill, step forms | `frus-widgets` (textinput, form, field, autofill, otpinput, stepform) | `input.md` | extended (custom and cross-field rules, `ErrorSummary`, autofill and input filters, `StepForm`); no separate forms page |
| Lists, grids, wheels, chat, file tree | `frus-widgets` | `lists.md` | extended (`ChatList`/`ChatComposer`, `FileTree`) |
| Navigation, scaffold, dialogs, menus, overlays | `frus-widgets` (router, scaffold, …) | `navigation.md` | extended (`Sidebar`) |
| Animation, motion, display widgets | `frus-core::animation`, `frus-widgets` | `animation.md` | extended (`RollingDigits`, `Marquee`, `KpiCard`) |
| Network, JSON | `frus-shell::net` | `network.md` | unchanged |
| Images, database, sign-in | `frus-shell` (image_cache, database, auth), `frus-image`, `frus-svg`, `frus-lottie` | `data.md` | unchanged |
| The system: dialogs, notifications, tray, taskbar, shortcuts, icon, permissions, prefs, secrets, haptics, network state | `frus-shell` | `platform.md` | extended: **the window** (size, memory, menu bar on the title line) |
| Windows and the desktop | `frus-shell` (window_memory, title_bar, instance) | one sentence (single instance) | folded into `platform.md` rather than a page of its own |
| Web specifics: build and host, address bar, text input, fonts, DOM accessibility bridge | `frus-shell` (address, history, web_editing, a11y_web) | `getting-started.md` (address bar only) | **new** `web.md` |
| Android and iOS specifics | `frus-shell` (android_*), CLI template | `getting-started.md` (setup) | **new** `mobile.md` |
| Running and hot reload | `frus-cli` (run, hot, keys), `frus-shell::hot` | scattered (getting-started, devtools, build-variants) | **new** `running.md` |
| Build modes, flavors, releases | `frus-cli` | `build-variants.md` | unchanged |
| Testing | `frus-shell::tester`, `frus-test` | `testing.md` | unchanged |
| DevTools, MCP | `frus-shell::devtools`, `frus-cli::mcp` | `devtools.md`, `mcp.md` | unchanged |
| Shaders, GPU passes | `frus-gpu`, `frus-shaders` | `shaders.md` | unchanged |
| Textures, native views, web views, video, camera, purchases | `frus-shell` | their pages | unchanged; `native-views.md`'s doctest holder fixed |
| Rendering and the frame pipeline (contributors) | `frus-core::scene`, `frus-gpu`, `frus-shell` (app, pacing, frame_stats), `frus-widgets` (relayout, paintcache) | `ARCHITECTURE.md` (stale in places) | **new** `rendering.md`; `ARCHITECTURE.md` corrected |
| Benchmarks, the demo, the examples | `frus-bench`, `frus-demo`, `frus-hello`, … | not user-facing | none needed |

Considered and not made a page of its own: **theming** (`style.md` already covers the scheme,
light and dark, widget themes and `ThemeExtension`, and `animation.md` the motion tokens);
**forms** (folded into `input.md`, where fields already are); **windows and the desktop**
(folded into `platform.md`, beside the tray and the taskbar).

## What changed

New pages: `components.md`, `screen.md`, `text.md`, `accessibility.md`, `localization.md`,
`running.md`, `web.md`, `mobile.md`, `rendering.md`. The index groups all thirty pages into
building screens, data and the system, and tools and shipping.

Corrections to existing pages, each checked against the code:

- `style.md` said `.max_lines(2).ellipsis()` cuts text after two lines. `Text::ellipsis` sets
  `wrap = false`, so it is always one line; two lines is
  `.wrap().max_lines(2).overflow(TextOverflow::Ellipsis)`.
- `style.md` said `Text` takes the theme's body style. `Text::new` inherits from its subtree,
  and otherwise is 16 px, regular, `on_surface`.
- `layout.md` said a `Scaffold` keeps content clear of the system's bars. It keeps its own bars
  clear; its body reaches the edges and is only told about the insets (`scaffold.rs`).
- `ARCHITECTURE.md` showed `view(&self, theme, w, h)` (now `view(&self, &Theme)`), listed only
  Vulkan, Metal, DX12 and WebGPU (GL ES and WebGL 2 are used too), counted 276 milestone notes
  and said they were in French.
- `crates/frus/src/lib.rs` included `native-views.md` without `#[cfg(doctest)]`, so the
  `GuideNativeViews` holder was a public, documented type of `frus`.

API: the guides name `LocationStrategy` (`FrusApp::location_strategy` takes it),
`location_of_link` and `link_among`, which `frus-shell` exports but `frus` did not, so an application depending only on `frus` could not
write them. `frus` re-exports them now.

## How the samples are checked

Every Rust sample of the new pages is a doctest: each page is included in `crates/frus/src/lib.rs`
under `#[cfg(doctest)]`, like the existing ones, so `cargo test -p frus --doc` compiles them all
and runs those with assertions. `frus-l10n` became a dev-dependency of `frus` for the Fluent
sample of `localization.md`. `rendering.md` has no Rust sample and is not included. The new
`platform.md` samples run under the platform-feature step of CI that already compiles that page.
The `toml` and `sh` blocks are not compiled; each was checked by hand against the CLI's usage
text and the template's `Cargo.toml.liquid`.

## Not done

- `getting-started.md`'s "State, in three sizes" snippets are still not compiled (one lacks
  imports, one is a fragment); `components.md` is now the compiled version, and the summary
  links there.
- Found while auditing, not fixed: `install_ambient` (`frus-shell/src/app.rs`) calls
  `Application::localizations()` before it installs the frame's locale, although its comment
  says the locale is installed first so that a table chosen by language can read it. An
  `Application` that picks its table with `frus::locale::of()` inside `localizations()` is one
  frame late after a language change. `localization.md` shows a table that reads the language
  in each entry instead, which is not affected.
- Found, not fixed: on the desktop and the web `MediaQuery::text_scaler` is always 1; the guides
  say so.
- `text.md` describes the `--unicodes` subsetting recipe from J567; it was not re-measured.
- No page was checked by a reader new to frus.
