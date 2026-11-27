// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus mcp` spoken to the way an agent's client speaks to it: the program started, JSON-RPC
//! written to its standard input and read from its standard output, a message a line
//! (milestone 854). A request that waits does not hold the others up, and a request the client
//! cancels stops and is not answered.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A `frus mcp` started in `dir`, its answers read on a thread.
struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    answers: Receiver<String>,
    /// Answers read while another was waited for.
    early: Vec<String>,
}

impl Mcp {
    fn start(dir: &Path, envs: &[(&str, &Path)]) -> Mcp {
        let mut command = Command::new(env!("CARGO_BIN_EXE_frus"));
        command
            .arg("mcp")
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("frus mcp starts");
        let out = child.stdout.take().unwrap();
        let (send, answers) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        let input = child.stdin.take();
        let mut mcp = Mcp {
            child,
            input,
            answers,
            early: Vec::new(),
        };
        mcp.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#);
        mcp.answer(1, Duration::from_secs(20));
        mcp.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        mcp
    }

    fn send(&mut self, line: &str) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
    }

    /// A tool's call, as request `id`.
    fn call(&mut self, id: u32, tool: &str, arguments: &str) {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
        ));
    }

    fn cancel(&mut self, id: u32) {
        self.send(&format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/cancelled","params":{{"requestId":{id},"reason":"the user stopped it"}}}}"#
        ));
    }

    /// The answer to request `id`, waited for up to `wait`; answers to others are kept.
    fn answer(&mut self, id: u32, wait: Duration) -> String {
        self.try_answer(id, wait)
            .unwrap_or_else(|| panic!("no answer to {id} in {wait:?}; others: {:?}", self.early))
    }

    fn try_answer(&mut self, id: u32, wait: Duration) -> Option<String> {
        if let Some(at) = self.early.iter().position(|a| id_of(a) == Some(id)) {
            return Some(self.early.remove(at));
        }
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.answers.recv_timeout(left) {
                Ok(line) if id_of(&line) == Some(id) => return Some(line),
                Ok(line) => self.early.push(line),
                Err(RecvTimeoutError::Timeout) => return None,
                Err(RecvTimeoutError::Disconnected) => panic!("frus mcp ended"),
            }
        }
    }

    /// Closes standard input, as a client that is done does, and waits for the server to end.
    fn finish(mut self) -> bool {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status.success();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        false
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// The id of an answer: the first `"id":` of the line, which is the message's own (the texts
/// inside have their quotes escaped).
fn id_of(line: &str) -> Option<u32> {
    let rest = &line[line.find("\"id\":")? + 5..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse::<f64>().ok().map(|n| n as u32)
}

/// What a fake application's DevTools saw.
#[derive(Default)]
struct Seen {
    /// A `tree` call arrived (and is held, unanswered).
    tree_asked: AtomicBool,
    /// The connection of the held `tree` call was closed by the server.
    tree_dropped: AtomicBool,
}

/// A fake application's DevTools: `status` and `hello` answered at once, each connection on a
/// thread of its own; `tree` held, unanswered, until the caller hangs up.
fn fake_devtools(seen: Arc<Seen>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let seen = seen.clone();
            std::thread::spawn(move || answer_devtools(stream, &seen));
        }
    });
    format!("http://127.0.0.1:{port}/?token=t0k3n")
}

fn answer_devtools(mut stream: TcpStream, seen: &Seen) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if line.trim().is_empty() {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length: ") {
            length = n.trim().parse().unwrap();
        }
        line.clear();
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body = String::from_utf8_lossy(&body);
    let result = if body.contains(r#""method":"status""#) {
        r#"{"app":"counter","device":"Windows"}"#
    } else if body.contains(r#""method":"hello""#) {
        r#"{"platform":"windows","size":{"width":400,"height":300},"devicePixelRatio":1}"#
    } else if body.contains(r#""method":"tree""#) {
        seen.tree_asked.store(true, Ordering::SeqCst);
        // Held: the answer never comes. The server hanging up ends the read.
        let mut rest = [0u8; 64];
        loop {
            match stream.read(&mut rest) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        seen.tree_dropped.store(true, Ordering::SeqCst);
        return;
    } else {
        r#"{}"#
    };
    let body = format!(r#"{{"result":{result}}}"#);
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn wait_for(what: &AtomicBool, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if what.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A directory of its own under the system's temporary one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("frus-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// **While a call waits on the application, the others are answered**, and a cancelled call
/// hangs up on the application and is never answered.
#[test]
fn a_call_that_waits_holds_nothing_up_and_a_cancelled_one_stops() {
    let seen = Arc::new(Seen::default());
    let uri = fake_devtools(seen.clone());
    let dir = scratch("waits");
    let mut mcp = Mcp::start(&dir, &[]);

    mcp.call(2, "attach_app", &format!(r#"{{"uri":"{uri}"}}"#));
    let attached = mcp.answer(2, Duration::from_secs(20));
    assert!(attached.contains("Attached to counter@"), "{attached}");

    // The tree is asked for, and the application does not answer.
    mcp.call(3, "get_widget_tree", "{}");
    assert!(wait_for(&seen.tree_asked, Duration::from_secs(20)));

    // Meanwhile: a status, which itself asks the application, and a ping.
    mcp.call(4, "app_status", "{}");
    mcp.send(r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#);
    let status = mcp.answer(4, Duration::from_secs(20));
    assert!(status.contains("is running on Windows"), "{status}");
    mcp.answer(5, Duration::from_secs(10));
    assert!(
        mcp.try_answer(3, Duration::ZERO).is_none(),
        "the tree is still waited for"
    );

    // Cancelled: the server stops waiting and hangs up on the application.
    mcp.cancel(3);
    assert!(
        wait_for(&seen.tree_dropped, Duration::from_secs(10)),
        "the cancelled call stopped waiting for the application"
    );
    // And it is never answered: a ping sent after it comes back, and still nothing for 3.
    mcp.send(r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#);
    mcp.answer(6, Duration::from_secs(10));
    assert_eq!(mcp.try_answer(3, Duration::from_secs(2)), None);

    assert!(mcp.finish(), "frus mcp ends cleanly when its input closes");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whether the process `pid` is still running.
fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// **`app_status` answers while `launch_app` waits for the build**, and cancelling the launch
/// ends the run it started, its build with it.
///
/// The application's build script writes its process id and sleeps: the build lasts as long as
/// the test needs it to.
#[test]
fn app_status_answers_while_launch_app_builds_and_a_cancelled_launch_ends_its_build() {
    let dir = scratch("launch");
    let app = dir.join("slowapp");
    std::fs::create_dir_all(app.join("src")).unwrap();
    std::fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"slowapp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(app.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        app.join("build.rs"),
        "fn main() {\n    let dir = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();\n    \
         std::fs::write(format!(\"{dir}/build.pid\"), std::process::id().to_string()).unwrap();\n    \
         std::thread::sleep(std::time::Duration::from_secs(300));\n}\n",
    )
    .unwrap();
    let target = dir.join("target");
    let mut mcp = Mcp::start(&app, &[("CARGO_TARGET_DIR", &target)]);
    let device = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    };
    mcp.call(
        2,
        "launch_app",
        &format!(r#"{{"device":"{device}","flags":["--no-hot"],"wait_seconds":600}}"#),
    );

    // The build is under way: its script has said who it is.
    let pid_file = app.join("build.pid");
    let deadline = Instant::now() + Duration::from_secs(300);
    while !pid_file.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("the build script ran")
        .trim()
        .parse()
        .unwrap();
    assert!(alive(pid));

    // While launch_app waits, app_status answers, and sees the run building.
    let asked = Instant::now();
    mcp.call(3, "app_status", "{}");
    let status = mcp.answer(3, Duration::from_secs(20));
    assert!(asked.elapsed() < Duration::from_secs(10));
    assert!(status.contains("building or starting"), "{status}");
    assert!(
        mcp.try_answer(2, Duration::ZERO).is_none(),
        "still launching"
    );

    // Cancelled: the run is ended, its build with it, and the launch is never answered.
    mcp.cancel(2);
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(!alive(pid), "the build script was ended with the run");
    mcp.call(4, "app_status", "{}");
    let status = mcp.answer(4, Duration::from_secs(20));
    assert!(status.contains("No app"), "{status}");
    assert_eq!(mcp.try_answer(2, Duration::from_secs(1)), None);

    assert!(mcp.finish());
    let _ = std::fs::remove_dir_all(&dir);
}
