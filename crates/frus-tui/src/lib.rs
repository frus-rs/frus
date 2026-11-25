// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The terminal workbench of frus: what `frus` opens when it is run with no command in a
//! terminal.
//!
//! One screen for the loop of developing an application — pick a device, run, read the log,
//! reload — and for what surrounds it, building and checking the machine:
//!
//! - **Run**: the devices (and the emulators that can be started), the runs with where each
//!   one is, and their log. `Enter` runs on the selected device, or on every device marked
//!   with `Space`. With hot reload, each saved file is patched into the running application
//!   and the log says so; `r` reloads a run without it, `R` restarts, `s` stops.
//! - **Build**: `frus build` for a target, its output as it comes and its result.
//! - **Doctor**: `frus doctor`'s report, and `f` for `frus doctor --fix`.
//!
//! Every key has a button in the key bar, and the mouse clicks tabs, devices and runs and
//! scrolls the panes.
//!
//! The workbench does nothing itself: it is a client of the tool's own commands, started as
//! processes — `frus daemon` for the devices, `frus run --machine` for each run, `frus build`
//! and `frus doctor` — the same commands an editor drives. Leaving it (`q`) stops every run,
//! and with it the applications it started.

mod client;
mod json;
mod state;
mod view;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;

pub use state::{
    App, Check, Device, Effect, Emulator, Focus, Input, JobKind, JobState, Line, LineKind, Msg,
    Pane, Row, Run, RunCommand, RunEvent, RunStatus, Tab, BUILD_TARGETS,
};
pub use view::{draw, Theme};

/// How long the workbench waits for a key before looking at its processes again.
const TICK: Duration = Duration::from_millis(50);

/// Opens the workbench in this terminal, and returns once the developer leaves it, every
/// process it started ended.
///
/// `frus` is the tool's program, whose commands the workbench runs; they run in the current
/// directory, which is the application's. The colours follow `NO_COLOR`.
pub fn workbench(frus: PathBuf) -> std::io::Result<()> {
    let theme = if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        Theme::plain()
    } else {
        Theme::default()
    };
    workbench_with(frus, &theme)
}

/// [`workbench`], in the colours of `theme`.
pub fn workbench_with(frus: PathBuf, theme: &Theme) -> std::io::Result<()> {
    let dir = std::env::current_dir()?;
    let mut app = App::new(project_name(&dir));
    let (send, receive) = mpsc::channel();
    let mut client = client::Client::new(frus, dir, send);
    client.start_daemon();

    let mut terminal = ratatui::try_init()?;
    // A panic puts the terminal back (ratatui's hook) and lets the mouse go.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        previous(info);
    }));
    let _ = execute!(std::io::stdout(), EnableMouseCapture);

    let result = run_loop(&mut terminal, &mut app, &mut client, &receive, theme);

    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let _ = std::panic::take_hook();
    let mut stderr = std::io::stderr();
    client.shutdown(|text| {
        let _ = writeln!(stderr, "{text}");
    });
    result
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    client: &mut client::Client,
    receive: &mpsc::Receiver<Msg>,
    theme: &Theme,
) -> std::io::Result<()> {
    let size = terminal.size()?;
    app.update(Msg::Resize(size.width, size.height));
    let mut dirty = true;
    loop {
        if dirty {
            terminal.draw(|frame| view::draw(frame, app, theme))?;
            dirty = false;
        }
        let mut msgs = Vec::new();
        if event::poll(TICK)? {
            // Everything already typed, before drawing again: a held key does not lag.
            loop {
                if let Some(msg) = to_msg(event::read()?) {
                    msgs.push(msg);
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        msgs.extend(receive.try_iter());
        msgs.extend(client.poll());
        for msg in msgs {
            dirty = true;
            for effect in app.update(msg) {
                if effect == Effect::Quit {
                    if client.live_runs() > 0 {
                        app.update(Msg::DaemonMessage {
                            text: "Stopping the runs…".into(),
                            error: false,
                        });
                        terminal.draw(|frame| view::draw(frame, app, theme))?;
                    }
                    return Ok(());
                }
                client.apply(effect);
            }
        }
    }
}

/// A terminal event, as the workbench reads it.
fn to_msg(event: Event) -> Option<Msg> {
    let input = match event {
        Event::Resize(width, height) => return Some(Msg::Resize(width, height)),
        // Windows reports a key's release too: only the press counts.
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Char(c) if ctrl => Input::Ctrl(c.to_ascii_lowercase()),
                KeyCode::Char(c) => Input::Char(c),
                KeyCode::Up => Input::Up,
                KeyCode::Down => Input::Down,
                KeyCode::Left => Input::Left,
                KeyCode::Right => Input::Right,
                KeyCode::PageUp => Input::PageUp,
                KeyCode::PageDown => Input::PageDown,
                KeyCode::Home => Input::Home,
                KeyCode::End => Input::End,
                KeyCode::Tab => Input::Tab,
                KeyCode::BackTab => Input::BackTab,
                KeyCode::Enter => Input::Enter,
                KeyCode::Esc => Input::Esc,
                _ => return None,
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => Input::Click(mouse.column, mouse.row),
            MouseEventKind::ScrollUp => Input::WheelUp(mouse.column, mouse.row),
            MouseEventKind::ScrollDown => Input::WheelDown(mouse.column, mouse.row),
            _ => return None,
        },
        _ => return None,
    };
    Some(Msg::Input(input))
}

/// The name of the application in `dir`, from its manifest: what the tab bar shows.
fn project_name(dir: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line
                .strip_prefix("name")
                .map(str::trim_start)
                .and_then(|rest| rest.strip_prefix('='))
            {
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventState, MouseEvent};

    #[test]
    fn a_key_is_read_once() {
        let key = |kind| {
            Event::Key(KeyEvent {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::NONE,
                kind,
                state: KeyEventState::NONE,
            })
        };
        assert_eq!(
            to_msg(key(KeyEventKind::Press)),
            Some(Msg::Input(Input::Char('r')))
        );
        assert_eq!(to_msg(key(KeyEventKind::Release)), None);
        let ctrl_c = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(to_msg(ctrl_c), Some(Msg::Input(Input::Ctrl('c'))));
        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(to_msg(click), Some(Msg::Input(Input::Click(3, 4))));
    }

    #[test]
    fn the_application_is_named_from_its_manifest() {
        let dir = std::env::temp_dir().join(format!("frus-tui-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\nname = \"no\"\n\n[package]\nname = \"my_app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(project_name(&dir), Some("my_app".into()));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(project_name(&dir), None);
    }
}
