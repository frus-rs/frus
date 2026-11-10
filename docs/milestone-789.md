# Milestone 789 — `frus mcp`: AI agents drive the application

## Objective

A coding agent that changes a frus application's interface cannot see what it changed. It
reads the code, guesses the result, and asks the developer to look. Milestone 788 gave every
debug run a protocol that finds widgets, taps, types, scrolls, takes screenshots and reads the
log. This milestone puts it in the hands of agents: `frus mcp`, a Model Context Protocol server,
whose tools run the application, look into it, act on it and bring the agent's own edits in,
with nothing to configure in a project made by `frus create`.

## Design

### A server inside `frus`, by hand

`frus mcp` speaks MCP over standard input and output: JSON-RPC 2.0, one message a line,
`initialize` (versions 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05: the client's when it is
one of them, the newest otherwise), `ping`, `tools/list`, `tools/call`, and the notifications,
which need no answer. `resources/list` and `prompts/list` answer empty lists for clients that ask
anyway. It is written by hand on the CLI's own JSON (`json.rs`), like `frus daemon`: `frus`
has no dependencies (milestone 652's rule), and the part of the protocol a tools-only server
uses is a few hundred lines. An SDK would have brought an async runtime and serde into a
program that is compiled on every developer's machine by `cargo install`.

Standard output carries the protocol and nothing else. The tool's own messages, which
`report::say` and `report::warn` print on standard output for a person, go to standard error
in this mode (`report::to_stderr`), and every program the server starts has its output
captured.

### Every tool goes through what exists

- **Running**: `launch_app` starts `frus run --machine` (the same program, `current_exe`) in
  the project's directory, with `-d <device>` and the `flags` given, and reads its events: the
  device (`app.start`), the build and the tool's messages (`app.progress`), the application's
  log (`app.log`), the DevTools address (`app.devTools`), reloads and restarts
  (`app.restarted`), the end (`app.stop`). Cargo's output, on `frus run`'s standard error, is
  kept with them, so a compile error reaches the agent through `get_logs` and through the
  reload that caused it. `stop_app`, `hot_reload` and `hot_restart` are `frus run`'s machine
  commands (`app.stop`, `app.restart`).
- **Looking and acting**: every other tool is a call of the DevTools protocol on the run's
  address (`POST /api` with the token): `tree`, `find`, `node`, `at`, `tap`, `scroll`, `type`,
  `setText`, `key`, `back`, `screenshot`, `frames`, `set`, `hello`, `logs`. So the agent sees
  exactly what the DevTools page sees, on every platform `frus run` runs on: the desktop
  through its socket, Android through `adb forward`, the web through the page's queue.
- **Checking**: `analyze` and `run_tests` run `frus analyze` and `frus test` in the project
  and return what they said.

A tool's failure (the application is not up, a widget is not there, the build failed) is a
result with `isError: true` and a sentence the agent can act on, not a protocol error: the
agent reads it and tries something else.

### Tools written for an agent

The names are short verbs (`tap`, `enter_text`, `get_widget_tree`), the descriptions say when
to use each and what comes back, and the server's `instructions` describe a session in five
lines. Three choices keep the agent's context small:

- **The tree as text.** `tree` answers a few hundred kilobytes of JSON for a real screen. The
  tool writes one line a widget, indented by depth: `Button #4f9a0c21d3b2e810 [24,310 120x40]
  key=7 text="Save" (Button, actions=tap)`, the semantics' label only when it differs from the
  words. `root`, `depth` and `limit` (250 by default) cut it, and the cut is said with how to
  see the rest. `find_widgets` is the cheaper way when the agent knows what it looks for.
- **Screenshots downscaled.** A phone's screenshot is 1080 by 2340 pixels. The tool returns it
  as MCP image content with its longest side at most `max_size` (1024 by default, 0 for the
  full size), each pixel the average of those it covers, and says how image pixels map to the
  logical pixels `tap` takes. `save_to` keeps the full PNG. `frus` has no image library: a PNG
  decoder (inflate, the five filters, every 8-bit colour type) and an encoder (per-row filter
  choice, LZ77 with the fixed Huffman codes) are in `png.rs`, 500 lines with their tests. A
  flat 400 by 800 screen encodes under 20 KB.
- **The log by number.** Each line has a number, and `since` returns only the newer ones: an
  agent polling the log does not read it again.

### Hot reload that says when the change is in

With hot reload, the default of `frus run`, a saved file is patched in by itself: there is
nothing to ask for, and the agent needs to know when its edit is in, or why not. `hot_reload`
takes the newest modification time of the project's `src/` and waits for the first patch or
error event at or after it; an edit already patched answers at once, and no edit since the
application started is said as such. Without hot reload (`--no-hot`), it sends `app.restart`
and waits for the rebuild's outcome. A failure returns the error lines (cargo's and the patch
engine's). After a restart, it waits for the application to answer again before returning, so
the next `tap` does not hit a closed socket.

Every call to the application also waits out a short absence: a run still building is waited
for (20 s, then the agent is told to call `app_status`), and an application that is not
connected yet, as during a restart, is asked again.

### Attaching to a run the developer started

A developer often has `frus run` going already. Each debug run now notes its DevTools in the
build's directory, `target/frus-devtools/run-<pid>.json`: `{uri, root, app, device, platform,
pid}`, readable by the user only on Unix since it holds the token, removed when the run ends
(`devtools::record`, `devtools::stop`). The target directory was chosen over the project's
because it is ignored by git already, and over a temporary directory because it is the user's
and the project's. `attach_app` without arguments finds the project's runs there (the note's
`root` matched against the project's), asks each `status`, removes the notes of runs that no
longer answer, and takes the newest. Every other tool attaches by itself when nothing was
launched. A run of another project, or with another target directory, is reached by its address
(`uri`).

The DevTools protocol gains `stop` (as `q`), so that `stop_app` can end an attached run too.

### Several applications

Each `launch_app` is a session with its own `frus run`, its own DevTools port and its own log:
an agent can run the desktop and the phone side by side. Every tool takes an `app_id` and acts
on the last session without it. When the client closes the server's input, every run the
server launched is stopped (`app.stop`, then the process tree killed if it lingers).

### New projects come with it

`frus create` writes a `.mcp.json` registering `frus mcp`, the file Claude Code reads from a
project, and the template's README says how to tell the agent about it in `CLAUDE.md` or
`AGENTS.md`. The file starts with a dot, which cargo leaves out of a package when it is not
listed; `frus-cli`'s `include` lists `/template/**`, and `cargo package --list` shows
`template/.mcp.json` in the crate. `docs/guide/mcp.md` gives the configuration for Claude Code,
VS Code (Copilot's agent mode), Cursor and any other stdio client.

## How it was tested

- **Unit tests** (`frus-cli`): the handshake (a version agreed, a notification unanswered, an
  unknown method and an unknown tool refused with their codes); every tool described, with an
  object schema, a unique short name, and handled; a DevTools address read; a run followed
  through its events (device, log, the address, reloads, a failure, hot reload found off, the
  end); a patch applied and a located compile error recognised; the tree written as indented
  lines (a subtree, a depth, a limit, an unknown root); frames summed up; base64 read back; long
  text clipped in the middle; an attached run driven through a DevTools server of the test's
  (find, tap, the switches named as the protocol names them, typing into a target, a wrong
  token refused); the notes of runs found, another project's left alone, a dead run's removed.
  `png`: images written and read back (RGB and RGBA), a flat screen compressed under 20 KB, the
  template's icons (another encoder's dynamic codes) read, a phone-sized screen averaged down
  to 473 by 1024, broken PNGs refused. `create`: the new project's `.mcp.json` names `frus mcp`.
- **Windows, end to end, through a scripted JSON-RPC client** (Python, `frus mcp` on its pipes)
  on a project made by `frus create --frus-path` with a text field added: `initialize`,
  `tools/list` (23 tools), `list_devices`, `launch_app`, `app_status`, `get_widget_tree` (19
  widgets), `find_widgets` "+", two `tap`s (the count read 2), `enter_text` into the field ("Hello
  Ada" on screen), `enter_text` with `replace` ("Hello Grace"), `press_key` Tab, `go_back`,
  `widget_at`, `screenshot` (saved full size, 800 by 600, 22 KB once encoded again; 1.9 MB as
  the application wrote it), `set_animation_speed`, `set_debug_options`, `get_frame_stats`
  across the automatic counting (115 frames), `get_logs`, then `hot_reload` after an edit of
  the sources: **with hot reload**, "Patched in 16.7 s", the edited words on screen at the next
  call and the count still 2; a broken edit answered with the compiler's located error
  (`src/counter.rs:78:39: error: expected `,``); the fix patched in again; `stop_app`, and the
  server's exit when its input closed. The same with `--no-hot`: the reload rebuilt and
  restarted the application in 24 s, a broken edit returned cargo's error, the application
  left as it was. Transcripts and screenshots are in the milestone's log directory.
- **Attaching**: a plain `frus run --no-hot` started apart; a `frus mcp` with nothing launched
  found its note, attached by itself on `find_widgets`, tapped "+" by its label (the count
  read 1), `attach_app` without arguments, and `stop_app` ended that `frus run` through the new
  `stop`; its note was gone after, and a stale note of a killed run was removed.
- **Claude Code**: a scratch directory with a `.mcp.json` naming this build of `frus`;
  `claude mcp list` showed `frus … ✓ Connected`, and `claude -p` listed the 23 tools
  (`mcp__frus__tap`, …) from the server.
- `cargo package --list -p frus-cli` includes `template/.mcp.json`; a project made by
  `frus create` has it.
PHONE

## Not done

NOTDONE
