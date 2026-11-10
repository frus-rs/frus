# AI agents: `frus mcp`

`frus mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server. With it, an AI
coding agent runs your application, reads its widget tree and its log, taps, scrolls and types,
looks at screenshots, and hot reloads its own edits: it checks a change in the running
application the way you would, instead of guessing from the code.

It speaks MCP over standard input and output, so any agent that takes a "stdio" server can use
it. It needs nothing more than `frus` itself.

## Adding it to your agent

A project made by `frus create` already has a `.mcp.json` at its root:

```json
{
  "mcpServers": {
    "frus": {
      "command": "frus",
      "args": ["mcp"]
    }
  }
}
```

Agents that read a project's `.mcp.json` (Claude Code does) offer the frus tools as soon as
they are started in the project's directory, after asking you once whether to trust the server.
For an older project, copy that file to its root. Elsewhere:

- **Claude Code**, without the file: `claude mcp add frus -- frus mcp` in the project's
  directory (`--scope project` writes the `.mcp.json` above instead).
- **VS Code (Copilot's agent mode)**: `.vscode/mcp.json` in the project:

  ```json
  {
    "servers": {
      "frus": { "type": "stdio", "command": "frus", "args": ["mcp"] }
    }
  }
  ```

- **Cursor**: `.cursor/mcp.json` in the project, with the same `mcpServers` object as
  `.mcp.json`.
- **Any other agent**: a stdio server whose command is `frus` and whose arguments are `mcp`,
  started in the project's directory.

The server works on the project in the directory it is started in; every tool that needs a
project also takes a `project_dir`. If `frus` is not on the agent's `PATH`, give its full path
as the command (`~/.cargo/bin/frus`).

To steer the agent towards it, a line in your `CLAUDE.md` or `AGENTS.md` is enough: "Check UI
changes in the running app with the frus tools: launch_app, then find_widgets, tap and
screenshot; hot_reload after each edit."

## The tools

| Tool | What it does |
| --- | --- |
| `list_devices` | What the application can run on: this computer, phones and emulators, browsers |
| `launch_app` | Builds and starts the application (`frus run`) on a `device`, with optional `flags`, and waits until it is up |
| `attach_app` | Reaches a `frus run` you started yourself, by its DevTools address or by finding it |
| `app_status` | The applications the server drives: building, running or stopped, the device, the address; waits for one to be up |
| `stop_app` | Stops the application and its `frus run` |
| `hot_reload` | Brings an edit in, the state kept: waits for the saved change to be patched in, and returns the compile errors if it could not be |
| `hot_restart` | Rebuilds and restarts from scratch |
| `get_logs` | The application's log and frus's messages, the build's errors included; `since` gives only the newer lines |
| `get_widget_tree` | The widgets on screen, one line each: type, id, box, key, words, semantics; `root`, `depth` and `limit` keep it small |
| `find_widgets` | Widgets by the words they paint, their semantics label, their type or their key |
| `get_widget` | One widget in detail: box, layout style, semantics, ancestors, children |
| `widget_at` | The widget at a point |
| `tap` | Taps a widget found as `find_widgets` finds it, or a point |
| `scroll` | Scrolls the area under a widget or a point by `dx`, `dy` |
| `enter_text` | Types into a field, key by key, or sets its whole value (`replace`) |
| `press_key` | `Enter`, `Tab`, `Escape`, arrows, `ctrl+a`… |
| `go_back` | The system's Back |
| `screenshot` | The application as an image, downscaled (`max_size`, 1024 pixels by default); `save_to` keeps the full PNG |
| `set_animation_speed` | Slower (or faster) animations: `0.1` is ten times slower |
| `set_debug_options` | Debug paint, the performance overlay, the brightness, the platform, the text size |
| `get_frame_stats` | Each frame's build, layout and paint, and render times, and the slow frames |
| `analyze` | `frus analyze`: clippy and rustfmt |
| `run_tests` | `frus test`: the tests, each failure with its place |

Coordinates are logical pixels from the top-left corner of the application's window, the same
as the boxes the tree shows. Widgets are found as the [DevTools protocol](devtools.md#the-protocol)
finds them: `text` and `label` match whole words unless `contains` is true, and when several
widgets match, the one on top is acted on.

A session, as an agent runs it:

```text
launch_app {}                              → frus-run-4242 is running on this computer, window 800x600
find_widgets {"text": "+"}                 → Button #4f9a0c21d3b2e810 center (370, 330) …
tap {"text": "+"}                          → Tapped at (370, 330) on #4f9a0c21d3b2e810.
enter_text {"text": "Ada", "target": {"label": "Your name"}}
screenshot {}                              → (the image)
   … the agent edits src/counter.rs …
hot_reload {}                              → Patched in 1.4 s: the running app has the change, its state kept.
stop_app {}
```

## Launching, or attaching to a run

`launch_app` runs `frus run --machine` in the project's directory, with hot reload on as
`frus run` has it by default. `device` is what `frus run -d` takes (`android`, `web`, an id from
`list_devices`), and `flags` are more `frus run` options: `["--no-hot"]` makes `hot_reload`
rebuild and restart with the state saved and restored, `["--release"]` runs a release build, which
has no DevTools, so only the log, restarts and stopping work. The first build can take minutes:
when the application is not up within `wait_seconds` (300 by default), `launch_app` says so and
`app_status` with a `wait_seconds` goes on waiting. Several applications can run at once, on
several devices; each tool takes an `app_id`, and acts on the last one launched without it.

When you already have `frus run` going, the agent can use it instead: every debug run notes
where its DevTools are, in `target/frus-devtools/run-<pid>.json` (in the build's directory,
removed when the run ends), and `attach_app` without arguments finds the run of the project's
application there. The tools attach by themselves when nothing was launched. `attach_app` also
takes the address `frus run` printed (`uri`), for a run of another project or another target
directory. Through an attached run the agent drives the application as through a launched one;
`stop_app` asks that `frus run` to stop.

## What it can see

The agent sees what DevTools sees, against the last frame on screen: the tree, the words,
the semantics, the boxes, the frames' times, the log. Screenshots come from the application on
the desktop and in a browser, and from the device on Android, where the image is the whole
screen with its status bar. A window behind another one still answers.

It works wherever `frus run` runs the application: the desktop, an Android device or emulator,
a browser.
