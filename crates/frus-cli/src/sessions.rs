// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Several devices at once** (milestone 791): `frus run -d all`, or `-d windows -d android`.
//!
//! Each device gets a run of its own: this program again, as `frus run --machine -d <id>` with
//! the rest of the command line as it was given. Each builds for its device, starts the
//! application there and follows its log, as a run on one device does — so a device behaves the
//! same alone and among others. What this run adds is the terminal they share:
//!
//! - every line a run reports comes here with its device in front: `[Windows] Restarted in 3.1 s.`;
//! - every key goes to every run: `r` reloads them all, `R` restarts them all, `q` quits them all,
//!   `p` turns on debug paint everywhere;
//! - for an editor (`--machine`), the runs' events pass through as they are — each run has its
//!   own `appId` — and a command goes to the run its `params.appId` names, or to every run.
//!
//! Two runs for the same platform build the same thing: cargo lets one build at a time in a
//! target directory, and the second finds the work done. Runs that patch the code (hot reload)
//! each keep their own copy of the sources and their own build (`FRUS_RUN_SESSION`), so that one
//! saved file is patched into every device without two engines writing the same files.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::devices::{self, Device, Kind};
use crate::json::{object, parse, Value};
use crate::platforms::AppPlatform;
use crate::project::Project;
use crate::report;
use crate::run::{self, Action, RunOptions};

/// The variable naming a run's own working directories when it runs beside others.
pub(crate) const SESSION_ENV: &str = "FRUS_RUN_SESSION";

/// The word `-d` takes for every device the application can run on.
pub(crate) const ALL: &str = "all";

/// `dir`, or its own directory below it when this run is one of several (see `SESSION_ENV`).
pub(crate) fn session_dir(dir: std::path::PathBuf) -> std::path::PathBuf {
    match std::env::var(SESSION_ENV) {
        Ok(slot) if !slot.is_empty() => dir.join(slot),
        _ => dir,
    }
}

/// Whether the command line asks for more than one run.
pub(crate) fn is_several(devices: &[String]) -> bool {
    devices.len() > 1 || devices.iter().any(|d| d.eq_ignore_ascii_case(ALL))
}

/// The platform an application is built for to run on `device`, None when frus cannot run there.
pub(crate) fn app_platform(device: &Device, web: bool) -> Option<AppPlatform> {
    match &device.kind {
        Kind::Host => Some(AppPlatform::host()),
        Kind::Simulated { .. } if web => Some(AppPlatform::Web),
        Kind::Simulated { .. } => Some(AppPlatform::host()),
        Kind::Android { .. } | Kind::Emulator { .. } => Some(AppPlatform::Android),
        Kind::Browser { .. } | Kind::WebServer => Some(AppPlatform::Web),
        Kind::IosSimulator { .. } => None,
    }
}

/// What `all` means: this computer and every Android device that is connected and usable, for
/// the platforms the application is made for. Browsers are left out — there are usually several,
/// and one is a `-d web` away — as are emulators that are not running, iOS simulators and
/// simulated devices.
pub(crate) fn all_devices(found: &[Device], made_for: Option<&[AppPlatform]>) -> Vec<Device> {
    found
        .iter()
        .filter(|d| d.unavailable.is_none())
        .filter(|d| matches!(d.kind, Kind::Host | Kind::Android { .. }))
        .filter(|d| {
            let platform = app_platform(d, false);
            match (made_for, platform) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(list), Some(p)) => list.contains(&p),
            }
        })
        .cloned()
        .collect()
}

/// The devices `-d` named, `all` expanded, each once.
pub(crate) fn resolve(
    found: &[Device],
    wanted: &[String],
    made_for: Option<&[AppPlatform]>,
) -> Result<Vec<Device>, String> {
    let mut chosen: Vec<Device> = Vec::new();
    for want in wanted {
        let more = if want.eq_ignore_ascii_case(ALL) {
            let all = all_devices(found, made_for);
            if all.is_empty() {
                return Err(format!(
                    "-d all found nothing to run on. The devices:\n\n{}",
                    devices::table(found)
                ));
            }
            all
        } else {
            vec![devices::select(found, Some(want))?]
        };
        for device in more {
            if !chosen.iter().any(|d| d.id == device.id) {
                chosen.push(device);
            }
        }
    }
    Ok(chosen)
}

/// The prefix of a device's lines: a short name, the same width for every run.
fn tags(devices: &[Device]) -> Vec<String> {
    let names: Vec<String> = devices
        .iter()
        .map(|d| {
            let name: String = d.name.chars().take(18).collect();
            format!("[{name}]")
        })
        .collect();
    let width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
    names.into_iter().map(|n| format!("{n:<width$}")).collect()
}

/// A directory name for a device's run: its id, with what a path cannot hold made `_`.
fn slot(device: &Device) -> String {
    device
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// What a run says, on the way to this one.
enum Line {
    Out(usize, String),
    Err(usize, String),
    Closed(usize),
}

/// One device's run.
struct Session {
    device: Device,
    tag: String,
    child: Child,
    stdin: Option<ChildStdin>,
    /// The run's `appId`, from its `app.start`.
    app_id: Option<String>,
    /// Whether it patches the code in place, from its `app.started`.
    hot: bool,
    /// The application is up on the device, from its `app.started`.
    started: bool,
    /// It said where its screenshot went, or that there is none.
    shot_said: bool,
    /// Its output is closed: it has ended.
    done: bool,
    /// Where its DevTools is served, from its `app.devTools`: each run serves its own, on a
    /// port of its own.
    devtools: Option<String>,
    failed: bool,
}

impl Session {
    /// Sends the run an editor's command, as one JSON line.
    fn send(&mut self, command: &Value) {
        if let Some(stdin) = self.stdin.as_mut() {
            if writeln!(stdin, "{}", command.to_json()).is_err() || stdin.flush().is_err() {
                self.stdin = None;
            }
        }
    }
}

/// `frus run` on several devices: one run each, sharing this terminal.
pub(crate) fn run_many(options: &RunOptions) -> Result<ExitCode, String> {
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    let project = Project::find(&here)?;
    let found = devices::discover();
    let made_for = project.app_platforms();
    let chosen = resolve(&found, &options.devices, made_for.as_deref())?;
    for device in &chosen {
        if let Some(platform) = app_platform(device, options.web) {
            project
                .check_platform(platform)
                .map_err(|e| format!("{}: {e}", device.name))?;
        }
    }
    if let [only] = chosen.as_slice() {
        if !options.machine {
            println!("Only {} to run on.", only.name);
        }
        let options = RunOptions {
            devices: vec![only.id.clone()],
            ..options.clone()
        };
        return run::run(&options);
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let tags = tags(&chosen);
    let (lines_in, lines) = mpsc::channel::<Line>();
    let mut sessions = Vec::new();
    for (index, device) in chosen.iter().enumerate() {
        let mut command = Command::new(&exe);
        command
            .args(["run", "--machine", "-d", &device.id])
            .args(&options.forwarded)
            .env(SESSION_ENV, slot(device))
            .current_dir(&here)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start the run on {}: {e}", device.name))?;
        follow(child.stdout.take(), index, &lines_in, false);
        follow(child.stderr.take(), index, &lines_in, true);
        sessions.push(Session {
            device: device.clone(),
            tag: tags[index].clone(),
            stdin: child.stdin.take(),
            child,
            app_id: None,
            hot: false,
            started: false,
            shot_said: false,
            done: false,
            devtools: None,
            failed: false,
        });
    }
    drop(lines_in);
    let names: Vec<&str> = chosen.iter().map(|d| d.name.as_str()).collect();
    // Standard output is JSON only for an editor: there each run says where it runs.
    if !options.machine {
        println!(
            "Running on {} devices: {}. Each is built for, started and followed on its own; \
             every key applies to all of them.",
            chosen.len(),
            names.join(", ")
        );
    }
    let result = if options.machine {
        relay_for_editor(&mut sessions, &lines)
    } else {
        drive(&mut sessions, &lines)
    };
    for session in &mut sessions {
        if session.child.try_wait().ok().flatten().is_none() {
            // Asked to stop, it had its chance: what is left is ended.
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline && session.child.try_wait().ok().flatten().is_none() {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = session.child.kill();
        }
        let _ = session.child.wait();
    }
    result
}

/// Reads a run's output into `lines`, a line at a time.
fn follow(
    stream: Option<impl std::io::Read + Send + 'static>,
    index: usize,
    lines: &Sender<Line>,
    error: bool,
) {
    let Some(stream) = stream else {
        return;
    };
    let send = lines.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let line = if error {
                Line::Err(index, line)
            } else {
                Line::Out(index, line)
            };
            if send.send(line).is_err() {
                return;
            }
        }
        if !error {
            let _ = send.send(Line::Closed(index));
        }
    });
}

/// A command for the runs, as an editor writes it.
fn command(method: &str, params: Value) -> Value {
    object([
        ("id", Value::from("frus-run")),
        ("method", method.into()),
        ("params", params),
    ])
}

/// For a person: each run's lines with its device in front, the keys to every run.
fn drive(sessions: &mut [Session], lines: &Receiver<Line>) -> Result<ExitCode, String> {
    let keys = crate::keys::listen();
    let mut keys_said = false;
    // The runs still to take a screenshot, and the one taking it now, since when.
    let mut shots: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let mut shot: Option<(usize, Instant)> = None;
    loop {
        // What the runs said, for a twentieth of a second at most: a log that never pauses
        // must not keep the keys waiting.
        let until = Instant::now() + Duration::from_millis(50);
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match lines.recv_timeout(left) {
                Ok(Line::Out(i, text)) => show(&mut sessions[i], &text),
                Ok(Line::Err(i, text)) => {
                    let text = crate::hot::plain(&text);
                    if !text.trim().is_empty() {
                        eprintln!("{} {text}", sessions[i].tag);
                    }
                }
                Ok(Line::Closed(i)) => sessions[i].done = true,
                Err(_) => break,
            }
        }
        if sessions.iter().all(|s| s.done) {
            break;
        }
        if !keys_said && sessions.iter().all(|s| s.done || s.started) {
            keys_said = true;
            report::say(&format!("\nKeys, for every device at once:\n{}", run::KEYS));
        }
        let shot_over = shot.is_none_or(|(i, since)| {
            sessions[i].shot_said || sessions[i].done || since.elapsed() > Duration::from_secs(15)
        });
        if shot_over {
            shot = None;
            if let Some(i) = shots.pop_front() {
                sessions[i].shot_said = false;
                sessions[i].send(&command("app.key", object([("key", "s".into())])));
                shot = Some((i, Instant::now()));
            }
        }
        let Some(action) = next_action(&keys) else {
            continue;
        };
        match action {
            Action::Quit => {
                for session in sessions.iter_mut() {
                    session.send(&command("app.stop", object([])));
                }
            }
            Action::Detach => {
                for session in sessions.iter_mut() {
                    session.send(&command("app.detach", object([])));
                }
            }
            Action::Help => report::say(run::KEYS),
            Action::Clear => run::clear_screen(),
            Action::Missing(why) => report::say(why),
            // Each run serves its own DevTools: every one that does is opened, named.
            Action::DevTools => {
                let served: Vec<(String, String)> = sessions
                    .iter()
                    .filter(|s| !s.done)
                    .filter_map(|s| Some((s.devtools.clone()?, s.device.name.clone())))
                    .collect();
                if served.is_empty() {
                    report::say(
                        "no run serves DevTools yet: it comes with a debug build, once the \
                         application runs",
                    );
                }
                for (address, device) in served {
                    crate::devtools::open_address(&address, Some(&device));
                }
            }
            Action::Reload | Action::Restart => {
                let full = action == Action::Restart;
                if !full && sessions.iter().any(|s| s.hot && !s.done) {
                    report::say(
                        "The devices with hot reload had each saved change patched in already; \
                         the others reload now.",
                    );
                }
                for session in sessions.iter_mut() {
                    session.send(&command(
                        "app.restart",
                        object([("fullRestart", full.into())]),
                    ));
                }
            }
            Action::Screenshot => {
                // One after the other: each run takes the next free `frus_NN.png`, which it
                // only knows once the one before has written its file.
                shots = (0..sessions.len()).filter(|&i| !sessions[i].done).collect();
                shot = None;
            }
            Action::App(_) => {
                let key = last_key_of(action);
                for session in sessions.iter_mut() {
                    session.send(&command("app.key", object([("key", key.as_str().into())])));
                }
            }
        }
    }
    let failed: Vec<&str> = sessions
        .iter()
        .filter(|s| s.failed)
        .map(|s| s.device.name.as_str())
        .collect();
    if failed.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Err(format!(
            "the run ended with an error on {}",
            failed.join(", ")
        ))
    }
}

/// The letter of an `App` action: the key the run on the other side takes.
fn last_key_of(action: Action) -> String {
    ('!'..='~')
        .find(|&key| run::action(key) == Some(action))
        .map(String::from)
        .unwrap_or_default()
}

/// The next key's action, if one was pressed.
fn next_action(keys: &Receiver<char>) -> Option<Action> {
    match keys.try_recv() {
        Ok(key) => run::action(key),
        Err(_) => None,
    }
}

/// A run's line, for a person: its event said with the device in front.
fn show(session: &mut Session, line: &str) {
    let tag = session.tag.clone();
    let Ok(value) = parse(line) else {
        if !line.trim().is_empty() {
            println!("{tag} {line}");
        }
        return;
    };
    let Some(event) = value.get("event").and_then(Value::as_str) else {
        // An answer to a command this run sent: nothing to show.
        return;
    };
    let params = value.get("params");
    let text = |key: &str| {
        params
            .and_then(|p| p.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let flag = |key: &str| {
        params
            .and_then(|p| p.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    match event {
        "app.start" => {
            session.app_id = Some(text("appId"));
        }
        "app.started" => {
            session.hot = flag("hotReload");
            session.started = true;
            let how = if session.hot {
                "running, with hot reload"
            } else {
                "running"
            };
            println!("{tag} {} is {how}.", session.device.name);
        }
        "app.progress" | "app.log" => {
            let message = if event == "app.log" {
                text("log")
            } else {
                text("message")
            };
            // "Screenshot written to…", "the screenshot did not arrive…", "the device took no
            // screenshot": the next run may take its own.
            if event == "app.progress" && message.to_ascii_lowercase().contains("screenshot") {
                session.shot_said = true;
            }
            if flag("error") {
                eprintln!("{tag} {message}");
            } else {
                println!("{tag} {message}");
            }
        }
        "app.devTools" => {
            let uri = text("uri");
            println!("{tag} DevTools: {uri} (v opens it)");
            session.devtools = Some(uri);
        }
        "app.restarted" => {
            let seconds = params
                .and_then(|p| p.get("seconds"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let what = if flag("fullRestart") {
                "Restarted"
            } else if session.hot {
                "Patched"
            } else {
                "Reloaded"
            };
            println!("{tag} {what} in {seconds:.1} s.");
        }
        "app.stop" => {
            let error = text("error");
            if error.is_empty() {
                println!("{tag} The run has ended.");
            } else {
                session.failed = true;
                eprintln!("{tag} The run has ended: {error}");
            }
        }
        _ => {}
    }
}

/// For an editor: the runs' events as they are, each with its `appId`; its commands to the run
/// they name, or to all of them.
fn relay_for_editor(sessions: &mut [Session], lines: &Receiver<Line>) -> Result<ExitCode, String> {
    let (commands_in, commands) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if commands_in.send(line).is_err() {
                return;
            }
        }
    });
    loop {
        let until = Instant::now() + Duration::from_millis(50);
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            match lines.recv_timeout(left) {
                Ok(Line::Out(i, text)) => {
                    let Ok(value) = parse(&text) else {
                        continue;
                    };
                    // A line with no event is the run's answer to a command, which this run
                    // answered already.
                    if let Some(event) = value.get("event").and_then(Value::as_str) {
                        if event == "app.start" {
                            sessions[i].app_id = value
                                .get("params")
                                .and_then(|p| p.get("appId"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        if event == "app.stop" {
                            let error = value
                                .get("params")
                                .and_then(|p| p.get("error"))
                                .is_some_and(|e| !matches!(e, Value::Null));
                            sessions[i].failed |= error;
                        }
                        report::raw_line(&text);
                    }
                }
                Ok(Line::Err(_, text)) => eprintln!("{text}"),
                Ok(Line::Closed(i)) => sessions[i].done = true,
                Err(_) => break,
            }
        }
        if sessions.iter().all(|s| s.done) {
            break;
        }
        while let Ok(line) = commands.try_recv() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() {
                continue;
            }
            let Ok(command) = parse(line) else {
                report::line(&object([
                    ("id", Value::Null),
                    ("error", "not a JSON command".into()),
                ]));
                continue;
            };
            let id = command.get("id").cloned().unwrap_or(Value::Null);
            let target = command
                .get("params")
                .and_then(|p| p.get("appId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut sent = 0;
            for session in sessions.iter_mut().filter(|s| !s.done) {
                if target.is_none() || target == session.app_id {
                    session.send(&command);
                    sent += 1;
                }
            }
            report::line(&if sent > 0 {
                object([("id", id), ("result", true.into())])
            } else {
                object([("id", id), ("error", "no run has that appId".into())])
            });
        }
    }
    if sessions.iter().any(|s| s.failed) {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn device(id: &str, kind: Kind, unavailable: Option<&str>) -> Device {
        let platform = match kind {
            Kind::Host => devices::Platform::Desktop,
            Kind::Android { .. } | Kind::Emulator { .. } => devices::Platform::Android,
            Kind::Simulated { .. } => devices::Platform::Simulated,
            Kind::IosSimulator { .. } => devices::Platform::Ios,
            _ => devices::Platform::Web,
        };
        Device {
            id: id.into(),
            name: id.to_uppercase(),
            platform,
            kind,
            detail: String::new(),
            unavailable: unavailable.map(str::to_string),
        }
    }

    fn found() -> Vec<Device> {
        vec![
            device("windows", Kind::Host, None),
            device(
                "phone1",
                Kind::Android {
                    serial: "phone1".into(),
                },
                None,
            ),
            device(
                "phone2",
                Kind::Android {
                    serial: "phone2".into(),
                },
                Some("unauthorized"),
            ),
            device(
                "pixel",
                Kind::Emulator {
                    avd: "pixel".into(),
                },
                None,
            ),
            device(
                "edge",
                Kind::Browser {
                    exe: PathBuf::from("edge"),
                },
                None,
            ),
            device("web-server", Kind::WebServer, None),
            device(
                "iphone-15",
                Kind::Simulated {
                    profile: "iphone-15".into(),
                },
                None,
            ),
        ]
    }

    fn ids(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|d| d.id.as_str()).collect()
    }

    /// `all` is this computer and the phones that can be used now: not a phone that has not been
    /// allowed, not an emulator that is not running, not the browsers nor the simulations.
    #[test]
    fn all_is_this_computer_and_the_usable_phones() {
        assert_eq!(ids(&all_devices(&found(), None)), ["windows", "phone1"]);
        let android_only = [AppPlatform::Android, AppPlatform::Web];
        assert_eq!(ids(&all_devices(&found(), Some(&android_only))), ["phone1"]);
        assert!(all_devices(&found(), Some(&[AppPlatform::Web])).is_empty());
    }

    #[test]
    fn devices_named_are_each_run_once() {
        let wanted =
            |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        let chosen = resolve(&found(), &wanted(&["all", "edge", "phone1"]), None).unwrap();
        assert_eq!(ids(&chosen), ["windows", "phone1", "edge"]);
        let chosen = resolve(&found(), &wanted(&["edge", "windows"]), None).unwrap();
        assert_eq!(ids(&chosen), ["edge", "windows"]);
        assert!(resolve(&found(), &wanted(&["nothing"]), None).is_err());
        assert!(
            resolve(&found(), &wanted(&["all"]), Some(&[AppPlatform::Web]))
                .unwrap_err()
                .contains("-d all found nothing")
        );
        assert!(is_several(&wanted(&["all"])));
        assert!(is_several(&wanted(&["a", "b"])));
        assert!(!is_several(&wanted(&["a"])));
        assert!(!is_several(&[]));
    }

    #[test]
    fn each_device_has_a_tag_of_one_width_and_a_directory_name() {
        let devices = vec![
            device("windows", Kind::Host, None),
            device("XMJ:5555", Kind::Android { serial: "x".into() }, None),
        ];
        assert_eq!(tags(&devices), ["[WINDOWS] ", "[XMJ:5555]"]);
        assert_eq!(slot(&devices[1]), "XMJ_5555");
    }

    #[test]
    fn a_device_is_built_for_its_platform() {
        let f = found();
        assert_eq!(app_platform(&f[0], false), Some(AppPlatform::host()));
        assert_eq!(app_platform(&f[1], false), Some(AppPlatform::Android));
        assert_eq!(app_platform(&f[4], false), Some(AppPlatform::Web));
        assert_eq!(app_platform(&f[6], true), Some(AppPlatform::Web));
        assert_eq!(app_platform(&f[6], false), Some(AppPlatform::host()));
    }

    #[test]
    fn an_app_key_is_sent_by_its_letter() {
        assert_eq!(last_key_of(Action::App("outlines")), "p");
        assert_eq!(last_key_of(Action::App("semantics")), "S");
    }

    #[test]
    fn a_run_s_events_are_said_with_its_device() {
        // The session's child is this test's own process stand-in: never started.
        let mut session = Session {
            device: device("windows", Kind::Host, None),
            tag: "[W]".into(),
            child: Command::new(std::env::current_exe().unwrap())
                .arg("--list")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
            stdin: None,
            app_id: None,
            hot: false,
            started: false,
            shot_said: false,
            done: false,
            devtools: None,
            failed: false,
        };
        show(
            &mut session,
            r#"{"event":"app.start","params":{"appId":"frus-run-7","deviceId":"windows"}}"#,
        );
        assert_eq!(session.app_id.as_deref(), Some("frus-run-7"));
        show(
            &mut session,
            r#"{"event":"app.started","params":{"appId":"frus-run-7","hotReload":true}}"#,
        );
        assert!(session.hot);
        // Each run serves its own DevTools, on a port of its own: `v` opens this one.
        show(
            &mut session,
            r#"{"event":"app.devTools","params":{"appId":"frus-run-7","uri":"http://127.0.0.1:9101/?token=t"}}"#,
        );
        assert_eq!(
            session.devtools.as_deref(),
            Some("http://127.0.0.1:9101/?token=t")
        );
        show(
            &mut session,
            r#"{"event":"app.stop","params":{"appId":"frus-run-7","error":"it crashed"}}"#,
        );
        assert!(session.failed);
        let _ = session.child.wait();
    }
}
