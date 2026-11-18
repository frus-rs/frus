# The frus guide

How to build an application with frus, one topic at a time. Every page is made of examples
you can paste: each one is a complete piece of Rust, and each is compiled — and the ones with
assertions run — by frus's own test suite, so none of them is out of date.

Start with [Starting a frus application](../getting-started.md): the smallest application,
the three ways to keep state, `async` work, and how to run on a phone. Then the pages, grouped from the first screen to shipping:

**Building screens**

| Page | What it covers |
| --- | --- |
| [Components and state](components.md) | Function components and hooks, `StatelessWidget`, `StatefulWidget` and its lifecycle, sharing state, keys, `async` work, restoring state |
| [Laying out a screen](layout.md) | Rows and columns, sharing the room, boxes, stacks, wrapping, scrolling |
| [The screen](screen.md) | Size and density, size classes, the safe area, the keyboard, the system bars, orientation, simulated devices |
| [Text, colour and the theme](style.md) | The theme's colours and type, light and dark, changing one widget or every widget of a kind, design tokens of your own, Apple-style widgets |
| [Text and fonts](text.md) | Rich text, selecting and copying, the bundled fonts and your own, subsetting, right to left, the reader's text size |
| [Taking input](input.md) | Text fields, controllers, checkboxes, switches, sliders, dropdowns, checking a form, autofill, forms in steps |
| [Lists and grids](lists.md) | Long lists, pull to refresh, swipe actions, grids, scrolling from code, wheels, conversations, file trees |
| [Screens, navigation and dialogs](navigation.md) | Routes, redirects, the `Scaffold`, dialogs and sheets, menus and panels, tabs, drawers, a sidebar |
| [Animation](animation.md) | Values that move to their new value, switching widgets, curves, motion from the theme, rolling numbers |
| [Accessibility](accessibility.md) | Screen readers on each platform, what widgets say for themselves, `Semantics`, the keyboard, the reader's settings, checking it |
| [Languages](localization.md) | Choosing the language, the application's words in Fluent files, the framework's words in seven languages, adding one |

**Data and the system**

| Page | What it covers |
| --- | --- |
| [Talking to a server](network.md) | Requests, JSON, loading a screen's data, sending a form |
| [Pictures, a database and signing in](data.md) | Network images cached on disk, SQLite with migrations and transactions, OAuth 2 / OpenID Connect sign-in with PKCE |
| [The system around the application](platform.md) | The window, file dialogs, dropped files, notifications, the tray, the taskbar, global shortcuts, the icon, permissions, preferences, secrets |
| [Android and iOS](mobile.md) | The Android project, the back button, links and shares, speed and battery, where iOS stands |
| [The web](web.md) | Building and hosting, the address bar, text input, fonts, screen readers, what a browser does not allow |
| [In-app purchases](purchases.md) | Products, buying, restoring, Google Play, a test store |
| [Playing video](video.md) | A clip from the application, a file or the network: the controller, the picture, the progress bar and the default controls; what each platform plays |
| [The camera and code scanning](camera.md) | The preview, pictures, flash, zoom and focus, the frames themselves; reading QR codes and barcodes |
| [Video, camera and other external pictures](textures.md) | Showing frames a decoder, a camera or a native view makes; pushing RGBA and YUV; Android's surface for `MediaCodec` and Java plugins |
| [Native views inside the page](native-views.md) | A web view, a map or any Android view inside a frus page; writing a factory for a plugin |
| [Web pages inside the page](webview.md) | `WebViewController` and `WebViewWidget`: load pages, history, JavaScript and its channels, navigation decisions, cookies |
| [Custom shaders and drawing on the GPU](shaders.md) | Fragment shaders in WGSL for boxes, paths, text and subtrees, their uniforms and images, animating them; a `GpuPass` drawing with frus's own `wgpu` |

**Tools and shipping**

| Page | What it covers |
| --- | --- |
| [Running and hot reload](running.md) | `frus run` on each device, its keys, hot reload and what it keeps, editors |
| [Testing](testing.md) | Testing the state and the logic, and what the screen looks like |
| [DevTools](devtools.md) | The live widget tree, each frame's cost, slow animations and the log, in the browser; the protocol behind it |
| [AI agents](mcp.md) | `frus mcp`: an MCP server through which a coding agent runs, inspects and drives the application |
| [Build modes, flavors and releases](build-variants.md) | Debug, profile and release; flavors; values given at build time; the version; App Bundles, one APK per processor, installers; signing |
| [Structuring an application](../app-structure.md) | Where each part of a growing application goes |
| [How a frame is made](rendering.md) | For contributors: the crates, one frame from event to pixels, when frames are drawn, where the time goes, the GPU backends |

Beyond the guide:

- **The widget gallery** — `frus-examples/widgets` — shows every widget with the code that
  builds it: 173 demos, in a browser at <https://frus-rs.github.io/frus/>.
- **The example applications** — `frus-examples/` — are complete projects: a counter, forms,
  sign-in, navigation, a client for a REST server, a WebSocket chat.
- **The API reference** — <https://docs.rs/frus> — has every type and method, most with an
  example.
