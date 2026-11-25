// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The workbench as a client of `frus`: what it shows comes from the tool's own commands,
//! started as processes, the way an editor uses them.
//!
//! - `frus daemon` lists the devices and the emulators, says when they come and go, and
//!   starts an emulator;
//! - `frus run --machine -d <device>` is one run: its JSON lines are the run's events, and it
//!   takes `app.restart` and `app.stop` on its input;
//! - `frus build <target>`, `frus doctor --machine` and `frus doctor --fix` run to their end,
//!   their output shown as it comes.
//!
//! Each process gets its own hidden console on Windows and its own process group elsewhere: a
//! program it starts cannot draw over the workbench, and ending it ends what it started.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::json::{parse, quote, Value};
use crate::state::{plain, Device, Effect, Emulator, JobKind, Msg, RunCommand, RunEvent};

/// The daemon's answers the workbench asked for, by the `id` of the question.
const GET_DEVICES: u64 = 1;
const GET_EMULATORS: u64 = 3;

/// A process and the streams of it still being read.
struct Process {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Its output streams not yet at their end: once none is, it is waited for.
    open: Arc<AtomicUsize>,
}

/// The processes the workbench started.
pub(crate) struct Client {
    frus: PathBuf,
    dir: PathBuf,
    send: Sender<Msg>,
    daemon: Option<Process>,
    runs: HashMap<usize, Process>,
    jobs: HashMap<JobKindKey, Process>,
    next_id: u64,
}

/// [`JobKind`] as a map key.
type JobKindKey = u8;

fn job_key(kind: JobKind) -> JobKindKey {
    match kind {
        JobKind::Build => 0,
        JobKind::Doctor => 1,
        JobKind::Fix => 2,
    }
}

fn job_kind(key: JobKindKey) -> JobKind {
    match key {
        0 => JobKind::Build,
        1 => JobKind::Doctor,
        _ => JobKind::Fix,
    }
}

impl Client {
    /// `frus` is the tool's program, `dir` the directory its commands run in.
    pub(crate) fn new(frus: PathBuf, dir: PathBuf, send: Sender<Msg>) -> Self {
        Client {
            frus,
            dir,
            send,
            daemon: None,
            runs: HashMap::new(),
            jobs: HashMap::new(),
            next_id: 100,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.frus);
        command.args(args).current_dir(&self.dir);
        // Colours are taken out anyway; asking for none spares the work.
        command.env("CARGO_TERM_COLOR", "never");
        own_group(&mut command);
        command
    }

    /// Starts `frus daemon` and asks it for the devices, to hear of their changes, and for
    /// the emulators.
    pub(crate) fn start_daemon(&mut self) {
        let mut command = self.command(&["daemon"]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.send.send(Msg::DaemonGone(format!(
                    "frus daemon did not start: {error}"
                )));
                return;
            }
        };
        let open = Arc::new(AtomicUsize::new(1));
        if let Some(stdout) = child.stdout.take() {
            let send = self.send.clone();
            let open = open.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    for msg in daemon_line(&line) {
                        if send.send(msg).is_err() {
                            return;
                        }
                    }
                }
                let _ = send.send(Msg::DaemonGone("frus daemon ended".into()));
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
        let mut daemon = Process {
            stdin: child.stdin.take(),
            child,
            open,
        };
        for (id, method) in [
            (GET_DEVICES, "device.getDevices"),
            (2, "device.enable"),
            (GET_EMULATORS, "emulator.getEmulators"),
        ] {
            write_line(
                &mut daemon,
                &format!(r#"{{"id":{id},"method":"{method}"}}"#),
            );
        }
        self.daemon = Some(daemon);
    }

    /// Carries out what the state asked for (all but [`Effect::Quit`]).
    pub(crate) fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::StartRun { key, device } => self.start_run(key, &device),
            Effect::Send { key, command } => {
                let id = self.id();
                let line = match command {
                    RunCommand::Reload | RunCommand::Restart => format!(
                        r#"{{"id":{id},"method":"app.restart","params":{{"fullRestart":{}}}}}"#,
                        command == RunCommand::Restart
                    ),
                    RunCommand::Stop => format!(r#"{{"id":{id},"method":"app.stop"}}"#),
                };
                if let Some(run) = self.runs.get_mut(&key) {
                    write_line(run, &line);
                }
            }
            Effect::LaunchEmulator(emulator) => {
                let id = self.id();
                if let Some(daemon) = &mut self.daemon {
                    write_line(
                        daemon,
                        &format!(
                            r#"{{"id":{id},"method":"emulator.launch","params":{{"emulatorId":{}}}}}"#,
                            quote(&emulator)
                        ),
                    );
                }
            }
            Effect::Build(target) => self.start_job(JobKind::Build, &["build", &target]),
            Effect::Doctor => self.start_job(JobKind::Doctor, &["doctor", "--machine"]),
            Effect::Fix => self.start_job(JobKind::Fix, &["doctor", "--fix"]),
            Effect::Cancel(kind) => {
                if let Some(job) = self.jobs.get_mut(&job_key(kind)) {
                    end_tree(&mut job.child);
                }
            }
            Effect::Quit => {}
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn start_run(&mut self, key: usize, device: &str) {
        let mut command = self.command(&["run", "--machine", "-d", device]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.send.send(Msg::Run {
                    key,
                    event: RunEvent::Stop {
                        error: Some(format!("frus run did not start: {error}")),
                    },
                });
                return;
            }
        };
        let open = Arc::new(AtomicUsize::new(2));
        if let Some(stdout) = child.stdout.take() {
            let send = self.send.clone();
            read_lines(stdout, open.clone(), move |line| {
                match run_line(&line) {
                    Some(event) => send.send(Msg::Run { key, event }).is_ok(),
                    // JSON the workbench has no use for: an answer to a command, an event it
                    // does not show.
                    None if parse(line.trim()).is_ok() => true,
                    // Not JSON: nothing of the protocol, but worth seeing.
                    None if !line.trim().is_empty() => send
                        .send(Msg::Run {
                            key,
                            event: RunEvent::Output(plain(&line)),
                        })
                        .is_ok(),
                    None => true,
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let send = self.send.clone();
            read_lines(stderr, open.clone(), move |line| {
                send.send(Msg::Run {
                    key,
                    event: RunEvent::Output(plain(&line)),
                })
                .is_ok()
            });
        }
        self.runs.insert(
            key,
            Process {
                stdin: child.stdin.take(),
                child,
                open,
            },
        );
    }

    fn start_job(&mut self, kind: JobKind, args: &[&str]) {
        let mut command = self.command(args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.send.send(Msg::JobLine {
                    job: kind,
                    text: format!("frus {} did not start: {error}", args.join(" ")),
                    error: true,
                });
                let _ = self.send.send(Msg::JobDone {
                    job: kind,
                    code: None,
                });
                return;
            }
        };
        let open = Arc::new(AtomicUsize::new(2));
        for (stream, error) in [
            (
                child
                    .stdout
                    .take()
                    .map(|s| Box::new(s) as Box<dyn Read + Send>),
                false,
            ),
            (
                child
                    .stderr
                    .take()
                    .map(|s| Box::new(s) as Box<dyn Read + Send>),
                true,
            ),
        ] {
            let Some(stream) = stream else {
                open.fetch_sub(1, Ordering::SeqCst);
                continue;
            };
            let send = self.send.clone();
            read_lines(stream, open.clone(), move |line| {
                send.send(Msg::JobLine {
                    job: kind,
                    text: plain(&line),
                    error,
                })
                .is_ok()
            });
        }
        self.jobs.insert(
            job_key(kind),
            Process {
                child,
                stdin: None,
                open,
            },
        );
    }

    /// What ended since the last look: each run and job whose output is read to its end, and
    /// whose process is gone.
    pub(crate) fn poll(&mut self) -> Vec<Msg> {
        let mut ended = Vec::new();
        let finished = |process: &mut Process| -> Option<Option<i32>> {
            if process.open.load(Ordering::SeqCst) > 0 {
                return None;
            }
            process.child.try_wait().ok().flatten().map(|s| s.code())
        };
        let runs: Vec<usize> = self.runs.keys().copied().collect();
        for key in runs {
            if let Some(code) = self.runs.get_mut(&key).and_then(finished) {
                self.runs.remove(&key);
                ended.push(Msg::Run {
                    key,
                    event: RunEvent::Exited(code),
                });
            }
        }
        let jobs: Vec<JobKindKey> = self.jobs.keys().copied().collect();
        for key in jobs {
            if let Some(code) = self.jobs.get_mut(&key).and_then(finished) {
                self.jobs.remove(&key);
                ended.push(Msg::JobDone {
                    job: job_kind(key),
                    code,
                });
            }
        }
        ended
    }

    /// The runs still going, for the goodbye.
    pub(crate) fn live_runs(&self) -> usize {
        self.runs.len()
    }

    /// Ends everything: each run asked to stop (its application with it) and waited for,
    /// the jobs ended, the daemon asked to leave. What does not end in time is ended by force,
    /// with whatever it started. `say` hears what is being waited for.
    pub(crate) fn shutdown(&mut self, mut say: impl FnMut(&str)) {
        for job in self.jobs.values_mut() {
            end_tree(&mut job.child);
        }
        for run in self.runs.values_mut() {
            write_line(run, r#"{"id":0,"method":"app.stop"}"#);
        }
        if let Some(daemon) = &mut self.daemon {
            write_line(daemon, r#"{"id":0,"method":"daemon.shutdown"}"#);
        }
        if !self.runs.is_empty() {
            say(&format!(
                "Stopping {} run{}…",
                self.runs.len(),
                if self.runs.len() == 1 { "" } else { "s" }
            ));
        }
        let mut all: Vec<&mut Process> = self
            .runs
            .values_mut()
            .chain(self.jobs.values_mut())
            .chain(self.daemon.as_mut())
            .collect();
        // A run stops its application and the patch engine: a few seconds at most.
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline
            && all
                .iter_mut()
                .any(|p| p.child.try_wait().ok().flatten().is_none())
        {
            std::thread::sleep(Duration::from_millis(100));
        }
        for process in &mut all {
            if process.child.try_wait().ok().flatten().is_none() {
                say("A process did not end in time: ended by force");
                end_tree(&mut process.child);
            }
            // Its input closed, the process has nothing left to read.
            process.stdin = None;
        }
        self.runs.clear();
        self.jobs.clear();
        self.daemon = None;
    }
}

fn write_line(process: &mut Process, line: &str) {
    if let Some(stdin) = &mut process.stdin {
        if writeln!(stdin, "{line}")
            .and_then(|()| stdin.flush())
            .is_err()
        {
            process.stdin = None;
        }
    }
}

/// Reads `stream` a line at a time on a thread of its own, handing each to `each` until it
/// says to stop; marks the stream closed at its end.
fn read_lines(
    stream: impl Read + Send + 'static,
    open: Arc<AtomicUsize>,
    mut each: impl FnMut(String) -> bool + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::new();
        // Bytes, not `lines()`: a program's output is not always UTF-8, and one bad byte must
        // not end the reading.
        while reader.read_until(b'\n', &mut bytes).is_ok_and(|n| n > 0) {
            let line = String::from_utf8_lossy(&bytes).into_owned();
            bytes.clear();
            if !each(line) {
                break;
            }
        }
        open.fetch_sub(1, Ordering::SeqCst);
    });
}

/// A process alone: its own hidden console on Windows, its own process group elsewhere.
fn own_group(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

/// Ends a process and every process it started.
fn end_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let ended = Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    #[cfg(unix)]
    let ended = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    #[cfg(not(any(windows, unix)))]
    let ended = {
        let _ = pid;
        false
    };
    if !ended {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// A device of the daemon's.
fn device(value: &Value) -> Option<Device> {
    Some(Device {
        id: value.str("id")?.to_string(),
        name: value.str("name").unwrap_or("").to_string(),
        platform: value.str("platform").unwrap_or("").to_string(),
        category: value.str("category").unwrap_or("").to_string(),
        detail: value.str("detail").unwrap_or("").to_string(),
        emulator: value.flag("emulator"),
        unavailable: if value.get("available").and_then(Value::as_bool) == Some(false) {
            Some(
                value
                    .str("unavailableReason")
                    .unwrap_or("unavailable")
                    .to_string(),
            )
        } else {
            None
        },
    })
}

/// A line of `frus daemon`, as what it means to the workbench.
pub(crate) fn daemon_line(line: &str) -> Vec<Msg> {
    let Ok(value) = parse(line) else {
        return Vec::new();
    };
    if let Some(event) = value.str("event") {
        let params = value.get("params");
        return match event {
            "device.added" => params
                .and_then(device)
                .map(Msg::DeviceAdded)
                .into_iter()
                .collect(),
            "device.removed" => params
                .and_then(|p| p.str("id"))
                .map(|id| Msg::DeviceRemoved(id.to_string()))
                .into_iter()
                .collect(),
            "daemon.logMessage" | "daemon.showMessage" => params
                .and_then(|p| {
                    Some(Msg::DaemonMessage {
                        text: p.str("message")?.to_string(),
                        error: p.str("level") == Some("error"),
                    })
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };
    }
    let id = value.get("id").and_then(Value::as_f64).map(|n| n as u64);
    if let Some(error) = value.str("error") {
        return vec![Msg::DaemonMessage {
            text: error.to_string(),
            error: true,
        }];
    }
    let list = || {
        value
            .get("result")
            .and_then(Value::as_array)
            .unwrap_or_default()
    };
    match id {
        Some(GET_DEVICES) => vec![Msg::Devices(list().iter().filter_map(device).collect())],
        Some(GET_EMULATORS) => vec![Msg::Emulators(
            list()
                .iter()
                .filter_map(|e| {
                    Some(Emulator {
                        id: e.str("id")?.to_string(),
                        name: e.str("name").unwrap_or("").to_string(),
                        platform: e.str("platformType").unwrap_or("").to_string(),
                    })
                })
                .collect(),
        )],
        _ => Vec::new(),
    }
}

/// A line of `frus run --machine`, as an event of the run. `None` for what is not one: an
/// answer to a command, or a line that is not JSON.
pub(crate) fn run_line(line: &str) -> Option<RunEvent> {
    let value = parse(line).ok()?;
    let event = value.str("event")?;
    let empty = Value::Object(Vec::new());
    let params = value.get("params").unwrap_or(&empty);
    let text = |key: &str| params.str(key).unwrap_or("").to_string();
    Some(match event {
        "app.start" => RunEvent::Start {
            device_name: text("deviceName"),
        },
        "app.progress" => RunEvent::Progress {
            message: plain(&text("message")),
            error: params.flag("error"),
        },
        "app.devTools" => RunEvent::DevTools(text("uri")),
        "app.started" => RunEvent::Started {
            hot: params.flag("hotReload"),
        },
        "app.log" => RunEvent::Log {
            text: plain(&text("log")),
            error: params.flag("error"),
        },
        "app.restarted" => RunEvent::Restarted {
            full: params.flag("fullRestart"),
            seconds: params.get("seconds").and_then(Value::as_f64).unwrap_or(0.0),
        },
        "app.stop" => RunEvent::Stop {
            error: params.str("error").map(str::to_string),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemon_is_read() {
        let devices = daemon_line(
            r#"{"id":1,"result":[{"id":"windows","name":"Windows","platform":"windows","category":"desktop","detail":"windows-x64","emulator":false,"available":true,"unavailableReason":null},{"id":"A1","name":"Phone","platform":"android","category":"mobile","detail":"","emulator":false,"available":false,"unavailableReason":"not authorised"}]}"#,
        );
        let [Msg::Devices(devices)] = devices.as_slice() else {
            panic!("{devices:?}");
        };
        assert_eq!(devices[0].name, "Windows");
        assert_eq!(devices[0].unavailable, None);
        assert_eq!(devices[1].unavailable.as_deref(), Some("not authorised"));

        assert_eq!(
            daemon_line(r#"{"event":"device.removed","params":{"id":"A1","name":"Phone"}}"#),
            [Msg::DeviceRemoved("A1".into())]
        );
        assert_eq!(
            daemon_line(
                r#"{"id":3,"result":[{"id":"Pixel_7","name":"Pixel 7","category":"mobile","platformType":"android"}]}"#
            ),
            [Msg::Emulators(vec![Emulator {
                id: "Pixel_7".into(),
                name: "Pixel 7".into(),
                platform: "android".into()
            }])]
        );
        assert_eq!(
            daemon_line(
                r#"{"event":"daemon.logMessage","params":{"level":"error","message":"adb is gone"}}"#
            ),
            [Msg::DaemonMessage {
                text: "adb is gone".into(),
                error: true
            }]
        );
        // `device.enable`'s answer and what is not JSON ask for nothing.
        assert!(daemon_line(r#"{"id":2,"result":true}"#).is_empty());
        assert!(daemon_line("garbage").is_empty());
    }

    #[test]
    fn a_run_is_read() {
        assert_eq!(
            run_line(
                r#"{"event":"app.start","params":{"appId":"frus-run-1","deviceId":"windows","deviceName":"This computer","directory":"D:\\app","supportsRestart":true}}"#
            ),
            Some(RunEvent::Start {
                device_name: "This computer".into()
            })
        );
        assert_eq!(
            run_line(r#"{"event":"app.started","params":{"appId":"x","hotReload":true}}"#),
            Some(RunEvent::Started { hot: true })
        );
        assert_eq!(
            run_line(
                r#"{"event":"app.log","params":{"appId":"x","log":"\u001b[31mred\u001b[0m","error":true}}"#
            ),
            Some(RunEvent::Log {
                text: "red".into(),
                error: true
            })
        );
        assert_eq!(
            run_line(
                r#"{"event":"app.restarted","params":{"appId":"x","fullRestart":false,"seconds":0.42}}"#
            ),
            Some(RunEvent::Restarted {
                full: false,
                seconds: 0.42
            })
        );
        assert_eq!(
            run_line(r#"{"event":"app.stop","params":{"appId":"x","error":null}}"#),
            Some(RunEvent::Stop { error: None })
        );
        assert_eq!(
            run_line(
                r#"{"event":"app.devTools","params":{"appId":"x","uri":"http://127.0.0.1:9100/"}}"#
            ),
            Some(RunEvent::DevTools("http://127.0.0.1:9100/".into()))
        );
        assert_eq!(run_line(r#"{"id":5,"result":true}"#), None);
        assert_eq!(run_line("   Compiling x"), None);
    }
}
