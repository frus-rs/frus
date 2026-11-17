// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Where a failed hot-reload build went wrong (milestone 816). The engine says that a patch
//! failed and what the compiler said, but not where: the compiler is asked again, and each error
//! is said with its place in the application's own files.
//!
//! That second compile runs **beside** the run's loop, never on it: the keys, the engine's lines
//! and the next patch are not held up while it works. Its answer comes back through a channel,
//! and an answer that is no longer wanted — a newer save came, or a patch went through — is
//! dropped, its compiler stopped.
//!
//! It is warm: the engine noted the compiler's command for the application's crate when it
//! built the program (its `.captured-args`), with every dependency it compiled, and that command
//! is run again with only the crate's metadata asked for. Only the application's own crate is
//! checked, in a second or two. Before the engine has built once, there is no such note, and
//! `cargo check` does it instead (cold, its first time).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use crate::json::Value;

/// What finds the errors: given the flag that says it is no longer wanted, the located errors,
/// or `None` when it was stopped or could not run.
pub(crate) type Runner = Arc<dyn Fn(&AtomicBool) -> Option<Vec<String>> + Send + Sync>;

/// The check under way, at most one.
struct Running {
    generation: u64,
    stop: Arc<AtomicBool>,
    /// The saves counted when it started: one more and its answer is about older sources.
    saves_at: u64,
}

/// Runs the error check on a thread of its own, one at a time; the loop polls for its answer.
pub(crate) struct ErrorCheck {
    runner: Runner,
    /// How many times the sources changed since the run began (see `run::mirror_sources`).
    saves: Arc<AtomicU64>,
    generation: u64,
    running: Option<Running>,
    send: Sender<(u64, Vec<String>)>,
    answers: Receiver<(u64, Vec<String>)>,
}

impl ErrorCheck {
    pub(crate) fn new(runner: Runner, saves: Arc<AtomicU64>) -> Self {
        let (send, answers) = channel();
        Self {
            runner,
            saves,
            generation: 0,
            running: None,
            send,
            answers,
        }
    }

    /// Starts a check, stopping the one under way: a newer failure supersedes it.
    pub(crate) fn start(&mut self) {
        self.cancel();
        self.generation += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.running = Some(Running {
            generation: self.generation,
            stop: stop.clone(),
            saves_at: self.saves.load(Ordering::SeqCst),
        });
        let runner = self.runner.clone();
        let send = self.send.clone();
        let generation = self.generation;
        std::thread::spawn(move || {
            if let Some(errors) = runner(&stop) {
                if !stop.load(Ordering::SeqCst) {
                    let _ = send.send((generation, errors));
                }
            }
        });
    }

    /// Stops the check under way, if any; whatever it finds is dropped. A patch that went through
    /// says the sources compile: there is nothing left to locate.
    pub(crate) fn cancel(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.store(true, Ordering::SeqCst);
        }
    }

    /// Whether a check is under way.
    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The errors the check under way found, once it has; never waits. A save made since it
    /// started stops it: its answer would be about sources that are gone.
    pub(crate) fn poll(&mut self) -> Option<Vec<String>> {
        if self
            .running
            .as_ref()
            .is_some_and(|r| self.saves.load(Ordering::SeqCst) != r.saves_at)
        {
            self.cancel();
        }
        while let Ok((generation, errors)) = self.answers.try_recv() {
            if self
                .running
                .as_ref()
                .is_some_and(|r| r.generation == generation)
            {
                self.running = None;
                return Some(errors);
            }
        }
        None
    }
}

impl Drop for ErrorCheck {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// The engine's profile for a platform (`desktop`, `wasm`, `android`): `desktop-dev`, or
/// `desktop-release` under `--release`.
pub(crate) fn engine_profile(platform: &str, release: bool) -> String {
    format!("{platform}-{}", if release { "release" } else { "dev" })
}

/// The compiler's command for the application's crate that the engine noted when it built the
/// program for `profile`: `<target>/dx/.captured-args/<package>-<triple>-<profile>-<hash>/<bin>.bin.json`,
/// the newest when there are several.
pub(crate) fn captured_command(target: &Path, profile: &str, bin: &str) -> Option<PathBuf> {
    let file = format!("{bin}.bin.json");
    let marker = format!("-{profile}-");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(target.join("dx").join(".captured-args"))
        .ok()?
        .filter_map(Result::ok)
    {
        if !entry.file_name().to_string_lossy().contains(&marker) {
            continue;
        }
        let path = entry.path().join(&file);
        let Ok(modified) = path.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(at, _)| modified > *at) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

/// The compiler's program and arguments for checking the crate, from the engine's note: the
/// same crate, dependencies and flags, with its errors one to a line, only its metadata
/// written (nothing linked), into `out_dir`, and without the engine's incremental cache, which
/// its own builds use.
pub(crate) fn check_arguments(captured: &Value, out_dir: &Path) -> Option<(String, Vec<String>)> {
    let args = captured.get("args")?.as_array()?;
    let mut args = args.iter().map(|a| a.as_str().map(str::to_string));
    let program = args.next()??;
    let args: Vec<String> = args.collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(args.len());
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out-dir" => {
                iter.next();
                out.push(arg);
                out.push(out_dir.display().to_string());
            }
            "-C" => {
                let value = iter.next().unwrap_or_default();
                if !dropped_codegen_option(&value) {
                    out.push(arg);
                    out.push(value);
                }
            }
            _ if arg.starts_with("--error-format") => out.push("--error-format=short".into()),
            _ if arg.starts_with("--json") => {}
            _ if arg.starts_with("--emit") => out.push("--emit=metadata".into()),
            _ if arg.starts_with("-C") && dropped_codegen_option(&arg[2..]) => {}
            _ => out.push(arg),
        }
    }
    Some((program, out))
}

/// The engine's options a check leaves out: its incremental cache, its kept temporary files.
fn dropped_codegen_option(value: &str) -> bool {
    value.starts_with("incremental=") || value.starts_with("save-temps")
}

/// The environment the engine's note gives the compiler, without what tied it to that build:
/// the job server of the cargo that is gone.
pub(crate) fn check_environment(captured: &Value) -> Vec<(String, String)> {
    captured
        .get("envs")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|pair| {
            let pair = pair.as_array()?;
            Some((pair.first()?.as_str()?, pair.get(1)?.as_str()?))
        })
        .filter(|(key, _)| !matches!(*key, "CARGO_MAKEFLAGS" | "MAKEFLAGS" | "MFLAGS"))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// What a check needs to know of the run.
#[derive(Clone)]
pub(crate) struct CheckPlan {
    /// The application's root, where its files are.
    pub root: PathBuf,
    /// The shadow package the engine builds.
    pub shadow: PathBuf,
    /// The engine's target directory.
    pub target: PathBuf,
    /// The program the engine builds (the library's name).
    pub bin: String,
    /// The engine's profile (see [`engine_profile`]).
    pub profile: String,
    /// The features the engine builds with.
    pub features: String,
}

/// The real check: the engine's noted command when there is one, `cargo check` otherwise; its
/// errors located in the application's files.
pub(crate) fn runner(plan: CheckPlan) -> Runner {
    Arc::new(move |stop: &AtomicBool| {
        let mut command = warm_command(&plan).unwrap_or_else(|| cold_command(&plan));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn().ok()?;
        let reader = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = stderr.read_to_string(&mut text);
                text
            })
        });
        crate::cargo::wait_or_stopped(&mut child, stop).ok()?;
        let text = reader.and_then(|r| r.join().ok()).unwrap_or_default();
        Some(
            text.lines()
                .filter_map(|line| crate::hot::located_error(line, &plan.shadow, &plan.root))
                .collect(),
        )
    })
}

/// The compiler run again as the engine ran it (see the module's documentation).
fn warm_command(plan: &CheckPlan) -> Option<Command> {
    let note = captured_command(&plan.target, &plan.profile, &plan.bin)?;
    let captured = crate::json::parse(&std::fs::read_to_string(note).ok()?).ok()?;
    let out_dir = plan.target.join("frus-check");
    std::fs::create_dir_all(&out_dir).ok()?;
    let (program, args) = check_arguments(&captured, &out_dir)?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&plan.shadow)
        .env_clear()
        .envs(check_environment(&captured));
    Some(command)
}

/// Before the engine has built once: `cargo check` of the shadow package, in a folder of its own.
fn cold_command(plan: &CheckPlan) -> Command {
    let mut command = Command::new("cargo");
    command
        .args([
            "check",
            "--message-format=short",
            "--bin",
            &plan.bin,
            "--features",
            &plan.features,
        ])
        .current_dir(&plan.shadow)
        .env("CARGO_TARGET_DIR", &plan.target);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// A runner that waits for its gate, then answers `errors`; it records whether it was told to
    /// stop.
    fn gated(errors: &[&str]) -> (Runner, Arc<Mutex<bool>>, Arc<AtomicBool>) {
        let gate = Arc::new(Mutex::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let errors: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let (g, s) = (gate.clone(), stopped.clone());
        let runner: Runner = Arc::new(move |stop: &AtomicBool| {
            while !*g.lock().unwrap() {
                if stop.load(Ordering::SeqCst) {
                    s.store(true, Ordering::SeqCst);
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Some(errors.clone())
        });
        (runner, gate, stopped)
    }

    fn poll_for(check: &mut ErrorCheck, wait: Duration) -> Option<Vec<String>> {
        let end = Instant::now() + wait;
        while Instant::now() < end {
            if let Some(found) = check.poll() {
                return Some(found);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    /// The freeze of milestone 816: a check that takes minutes left the loop without keys.
    /// Starting one returns at once, and the loop polls while it works.
    #[test]
    fn a_check_does_not_hold_the_loop() {
        let (runner, gate, _) = gated(&["src/a.rs:1:1: error: no"]);
        let mut check = ErrorCheck::new(runner, Arc::default());
        let started = Instant::now();
        check.start();
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(check.is_running());
        assert_eq!(
            check.poll(),
            None,
            "nothing yet: the check is still at work"
        );
        *gate.lock().unwrap() = true;
        assert_eq!(
            poll_for(&mut check, Duration::from_secs(5)),
            Some(vec!["src/a.rs:1:1: error: no".to_string()])
        );
        assert!(!check.is_running());
    }

    /// A patch that goes through says the sources compile: the check is stopped, its answer
    /// never said.
    #[test]
    fn a_patch_drops_the_check() {
        let (runner, gate, stopped) = gated(&["src/a.rs:1:1: error: stale"]);
        let mut check = ErrorCheck::new(runner, Arc::default());
        check.start();
        check.cancel();
        let end = Instant::now() + Duration::from_secs(5);
        while !stopped.load(Ordering::SeqCst) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            stopped.load(Ordering::SeqCst),
            "the compiler is told to stop"
        );
        *gate.lock().unwrap() = true;
        assert_eq!(poll_for(&mut check, Duration::from_millis(200)), None);
    }

    /// A save made while the check works makes its answer about sources that are gone.
    #[test]
    fn a_newer_save_drops_the_check() {
        let (runner, gate, stopped) = gated(&["src/a.rs:1:1: error: stale"]);
        let saves = Arc::new(AtomicU64::new(3));
        let mut check = ErrorCheck::new(runner, saves.clone());
        check.start();
        saves.fetch_add(1, Ordering::SeqCst);
        assert_eq!(check.poll(), None);
        assert!(!check.is_running());
        // The compiler is told to stop: seen before the gate opens, or the runner could finish
        // first and the test would race it.
        let end = Instant::now() + Duration::from_secs(5);
        while !stopped.load(Ordering::SeqCst) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            stopped.load(Ordering::SeqCst),
            "the compiler is told to stop"
        );
        *gate.lock().unwrap() = true;
        assert_eq!(poll_for(&mut check, Duration::from_millis(200)), None);
    }

    /// A second failure supersedes the first check: only the newest answer is said.
    #[test]
    fn only_the_newest_check_answers() {
        let calls = Arc::new(AtomicU64::new(0));
        let c = calls.clone();
        let runner: Runner = Arc::new(move |stop: &AtomicBool| {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // The first is slow, and answers even though it was told to stop.
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Some(vec!["old".to_string()]);
            }
            Some(vec!["new".to_string()])
        });
        let mut check = ErrorCheck::new(runner, Arc::default());
        check.start();
        // The first runner is under way before the second starts: the threads could otherwise
        // run in either order, and the slow answer would fall to the newest check.
        let end = Instant::now() + Duration::from_secs(5);
        while calls.load(Ordering::SeqCst) == 0 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
        check.start();
        assert_eq!(
            poll_for(&mut check, Duration::from_secs(5)),
            Some(vec!["new".to_string()])
        );
        assert_eq!(poll_for(&mut check, Duration::from_millis(100)), None);
    }

    #[test]
    fn the_engine_profile_follows_the_mode() {
        assert_eq!(engine_profile("desktop", false), "desktop-dev");
        assert_eq!(engine_profile("wasm", true), "wasm-release");
    }

    /// The engine's note, turned into a check of the crate alone: errors one to a line, only
    /// metadata, into a folder of its own, without the engine's incremental cache.
    #[test]
    fn the_noted_command_becomes_a_check() {
        let note = crate::json::parse(
            r#"{"args":["/bin/rustc","--crate-name","app","src/lib.rs","--error-format=json",
            "--json=diagnostic-rendered-ansi,artifacts","--crate-type","bin","--emit=dep-info,link",
            "-C","embed-bitcode=no","-C","incremental=/t/inc","--out-dir","/t/deps",
            "--extern","frus=/t/deps/libfrus.rlib","-Csave-temps=true","-Clink-dead-code"],
            "envs":[["CARGO_PKG_NAME","app"],["CARGO_MAKEFLAGS","-j --jobserver-auth=x"]]}"#,
        )
        .unwrap();
        let (program, args) = check_arguments(&note, Path::new("/t/check")).unwrap();
        assert_eq!(program, "/bin/rustc");
        assert_eq!(
            args,
            [
                "--crate-name",
                "app",
                "src/lib.rs",
                "--error-format=short",
                "--crate-type",
                "bin",
                "--emit=metadata",
                "-C",
                "embed-bitcode=no",
                "--out-dir",
                &Path::new("/t/check").display().to_string(),
                "--extern",
                "frus=/t/deps/libfrus.rlib",
                "-Clink-dead-code",
            ]
        );
        assert_eq!(
            check_environment(&note),
            [("CARGO_PKG_NAME".to_string(), "app".to_string())]
        );
    }

    /// The note is the profile's, the newest of them.
    #[test]
    fn the_note_of_the_profile_is_found() {
        let dir = std::env::temp_dir().join(format!("frus-locate-{}", std::process::id()));
        let notes = dir.join("dx").join(".captured-args");
        for name in [
            "app-x86_64-pc-windows-msvc-desktop-dev-1",
            "app-wasm32-unknown-unknown-wasm-dev-2",
        ] {
            std::fs::create_dir_all(notes.join(name)).unwrap();
            std::fs::write(notes.join(name).join("app.bin.json"), "{}").unwrap();
        }
        assert_eq!(
            captured_command(&dir, "wasm-dev", "app"),
            Some(
                notes
                    .join("app-wasm32-unknown-unknown-wasm-dev-2")
                    .join("app.bin.json")
            )
        );
        assert_eq!(captured_command(&dir, "android-dev", "app"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
