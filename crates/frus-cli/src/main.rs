// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus`, the command-line tool:
//!
//! ```text
//! frus create my_app        # a new application
//! frus devices              # what it can run on, now
//! frus run -d <device>      # build it, start it there, show its log
//! frus build apk|web|desktop
//! ```
//!
//! It drives the tools an application already uses — cargo, cargo-apk, adb, wasm-bindgen — and
//! adds no build system of its own: everything it does can be done by hand.

mod analyze;
mod build;
mod bundle;
mod cargo;
mod clean;
mod config_changes;
mod create;
mod daemon;
mod devices;
mod devlink;
mod doctor;
mod emulators;
mod hot;
mod json;
mod keys;
mod permissions;
mod platforms;
mod project;
mod report;
mod run;
mod serve;
mod sessions;
mod setup;
mod size;
mod socket;
mod template;
mod testing;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use create::{Created, Git, Options, DEFAULT_ORG};

const USAGE: &str = "\
frus — the frus command-line tool

Usage:
  frus create <directory> [options]   start a new application
  frus devices                        list what the application can run on
  frus run [options] [-- <args>]      build it, start it on a device, show its log
  frus build <apk|web|desktop>        build what you ship, in release
  frus doctor [-v] [--fix]            what this machine has for frus, and what it lacks
  frus emulators [--launch <id>|--create]  the emulators: list, start, make
  frus clean                          delete the application's builds
  frus test [<filter>] [-v]           the tests, said briefly: a summary, each failure at its place
  frus analyze [--fix]                clippy and rustfmt, everything to fix listed together
  frus size <target|file>             what the shipped binary is made of, crate by crate
  frus daemon                         JSON lines for an editor: devices, as they come and go
  frus help [<command>]               print this, or a command's options
  frus --version                      print the version
";

const RUN_USAGE: &str = "\
frus run — build the application, start it on a device, and show its log here

Usage:
  frus run [options] [-- <args for the application>]

Run from the application's directory. Its log follows in this terminal, on every
platform: the desktop's output, Android's logcat, the browser's console.

Options:
  -d, --device <device>    where to run: a device's id or name from `frus devices`,
                           or `android`, `web`, `desktop` (default: this computer).
                           Given more than once, or `all` (this computer and every
                           connected phone), it runs on each at once: one build per
                           platform, every log here with its device in front, and
                           every key (r, R, q…) for all of them
      --release            a release build (default: debug)
      --port <port>        the port the web build is served on (default: 8080)
      --no-hot             restart to reload (`r`) instead of patching the running
                           application; hot reload is the default for a debug build
                           (desktop, web, Android): each saved change is patched in,
                           every state kept (its engine is installed the first time)
      --hot                hot reload even with --release, and an error rather than
                           restarts where it cannot run
      --web                with a simulated device (`-d iphone-15`): in a browser
                           instead of a desktop window
      --machine            JSON lines for an editor instead of text (see `frus daemon`)
  -h, --help               print this

Examples:
  frus run
  frus run --no-hot
  frus run -d android
  frus run -d edge --release
  frus run -d iphone-15
  frus run -d iphone-15 --web
  frus run -d all
  frus run -d windows -d android -d edge
";

const EMULATORS_USAGE: &str = "\
frus emulators — the emulators that can be started, starting one, making one

Usage:
  frus emulators                          list them
  frus emulators --launch <id> [--cold]   start one (--cold: without its saved state)
  frus emulators --create [--name <name>] make an Android emulator, installing what it
                                          needs (a system image if none is there)
";

const DOCTOR_USAGE: &str = "\
frus doctor — what this machine has for building frus applications, and what it lacks

Usage:
  frus doctor [options]

Looks at Rust and its targets, what the desktop build needs, the Android SDK and its
tools, the web build, the hot-reload engine, iOS, VS Code and the devices. Changes
nothing unless --fix is given.

Options:
  -v, --verbose            everything that was found, not only what is wrong
      --fix                install what is missing (Rust targets, the Android SDK and
                           its tools, cargo-apk, the hot-reload engine)
      --machine            JSON, for an editor
  -h, --help               print this
";

const BUILD_USAGE: &str = "\
frus build — build what you ship, for one platform

Usage:
  frus build <target> [--debug]

Targets:
  apk        the Android package (cargo-apk; a release needs a signing key)
  web        the site: web/, with the application compiled into web/pkg/
  desktop    the program for this computer (also: windows, linux, macos)

Options:
      --debug             a debug build instead of release
      --analyze-size      what the release binary is made of, crate by crate, and the
                          fonts and assets inside it: a table here, the whole as JSON
                          (see `frus help size`)
      --compare <json>    with --analyze-size: what changed since that analysis
  -h, --help              print this
";

const SIZE_USAGE: &str = "\
frus size — what the shipped binary is made of

Usage:
  frus size <apk|web|desktop> [--compare <json>]   build it with symbols, then measure
  frus size <file> [--compare <json>]              measure a binary already built:
                                                   a .so, a .wasm, an .exe with its .map

The release is built again with its symbols kept, in target/frus-size/ (the build you
ship is left as it is), and each byte of code and read-only data is put down to the
crate it comes from. The fonts frus bundles and the files of assets/ found inside are
listed with their sizes. The whole analysis is written to target/frus-size/<platform>-NN.json;
`--compare` with an earlier one says what grew and what shrank.

`frus build <target> --analyze-size` is the same as `frus size <target>`.
How to make an application smaller: the guide's \"Shipping\" (docs/getting-started.md).

Examples:
  frus size desktop
  frus build apk --analyze-size
  frus size web --compare target/frus-size/web-01.json
";

const DEVICES_USAGE: &str = "\
frus devices — list what the application can run on

This computer, the Android devices adb sees (USB debugging on), the browsers
installed here, and a web server any other browser can open. The id in the
second column is what `frus run -d` takes.
";

const CREATE_USAGE: &str = "\
frus create — start a new application for desktop, Android and the web

Usage:
  frus create <directory> [options]

The directory is made if it is not there. In one that has files already, only the
template's missing files are written and nothing there is changed: `frus create .`
in an application brings back what was deleted (web/, icons, the desktop binary),
keeps its manifest and code, and takes its name from its Cargo.toml.
`frus create --platforms web .` adds a platform to an application: its files, and its
section of Cargo.toml.

A new application's name is the directory's own unless --name gives one:
lowercase letters, digits, `_` and `-`.

Options:
      --name <name>        the package's name (default: the directory's name)
      --org <org>          the reverse domain the Android package starts with
                           (default: com.example; the package is <org>.<name>)
      --platforms <list>   what it is made for, comma-separated: android, web,
                           windows, macos, linux, or desktop for the three
                           (default: all of them)
      --deeplink <link>    the links that open it: a scheme of its own (`myapp`) or
                           a host whose https links it opens (`https://example.com`)
      --description <text> one line saying what it is (Cargo.toml, the web page)
      --frus-path <dir>    build against a checkout of frus, not the published crates
      --no-git             do not make the directory a git repository
  -h, --help               print this

Examples:
  frus create my_app --org com.acme
  frus create my_app --platforms android,web --deeplink myapp
  frus create .                        # in an application: restore missing files
  frus create --platforms web .        # in an application: add the web
";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    CreateHelp,
    RunHelp,
    BuildHelp,
    DevicesHelp,
    Version,
    Create(Options),
    Devices,
    Daemon,
    Doctor(doctor::Options),
    DoctorHelp,
    Emulators(emulators::Action),
    EmulatorsHelp,
    Clean,
    Test {
        filter: Option<String>,
        verbose: bool,
    },
    Analyze {
        fix: bool,
    },
    Run(run::RunOptions),
    Build(build::Target, cargo::Mode),
    /// `frus build <target> --analyze-size`, `frus size` (milestone 791).
    Size(size::Options),
    SizeHelp,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse(args) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::CreateHelp) => {
            print!("{CREATE_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::RunHelp) => {
            print!("{RUN_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::BuildHelp) => {
            print!("{BUILD_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::DevicesHelp) => {
            print!("{DEVICES_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("frus {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Command::Devices) => {
            let found = devices::discover();
            println!("{} found:\n", plural(found.len(), "device"));
            print!("{}", devices::table(&found));
            ExitCode::SUCCESS
        }
        Ok(Command::Run(options)) => outcome(run::run(&options)),
        Ok(Command::Daemon) => outcome(daemon::daemon()),
        Ok(Command::Doctor(options)) => doctor::doctor(options),
        Ok(Command::Emulators(action)) => outcome(emulators::emulators(action)),
        Ok(Command::Clean) => outcome(clean::clean()),
        Ok(Command::Test { filter, verbose }) => outcome(testing::test(filter.as_deref(), verbose)),
        Ok(Command::Analyze { fix }) => outcome(analyze::analyze(fix)),
        Ok(Command::EmulatorsHelp) => {
            print!("{EMULATORS_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::DoctorHelp) => {
            print!("{DOCTOR_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Build(target, mode)) => outcome(build::build(target, mode)),
        Ok(Command::Size(options)) => outcome(size::size(&options)),
        Ok(Command::SizeHelp) => {
            print!("{SIZE_USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Command::Create(options)) => match create::create(&options) {
            Ok(created) => {
                print!("{}", report(&created));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("error: {error}\n\nRun `frus help` for the usage.");
            ExitCode::from(2)
        }
    }
}

fn parse(args: Vec<OsString>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };
    match command.to_str() {
        Some("-h" | "--help") => Ok(Command::Help),
        Some("help") => match args.next().as_ref().and_then(|a| a.to_str()) {
            None => Ok(Command::Help),
            Some("create") => Ok(Command::CreateHelp),
            Some("run") => Ok(Command::RunHelp),
            Some("build") => Ok(Command::BuildHelp),
            Some("devices") => Ok(Command::DevicesHelp),
            Some("doctor") => Ok(Command::DoctorHelp),
            Some("emulators") => Ok(Command::EmulatorsHelp),
            Some("size") => Ok(Command::SizeHelp),
            Some(other) => Err(format!("no command `{other}`")),
        },
        Some("-V" | "--version" | "version") => Ok(Command::Version),
        Some("create") => parse_create(args),
        Some("devices") => match args.next().as_ref().and_then(|a| a.to_str()) {
            None => Ok(Command::Devices),
            Some("-h" | "--help") => Ok(Command::DevicesHelp),
            Some(other) => Err(format!("frus devices takes no `{other}`")),
        },
        Some("run") => parse_run(args),
        Some("daemon") => Ok(Command::Daemon),
        Some("doctor") => parse_doctor(args),
        Some("emulators") => parse_emulators(args),
        Some("test") => {
            let mut filter = None;
            let mut verbose = false;
            for arg in args {
                match arg.to_str() {
                    Some("-v" | "--verbose") => verbose = true,
                    Some(text) if !text.starts_with('-') && filter.is_none() => {
                        filter = Some(text.to_string())
                    }
                    _ => {
                        return Err(format!(
                            "frus test has no option `{}`",
                            arg.to_string_lossy()
                        ))
                    }
                }
            }
            Ok(Command::Test { filter, verbose })
        }
        Some("analyze") => match args.next().as_ref().and_then(|a| a.to_str()) {
            None => Ok(Command::Analyze { fix: false }),
            Some("--fix") => Ok(Command::Analyze { fix: true }),
            Some(other) => Err(format!("frus analyze has no option `{other}`")),
        },
        Some("clean") => match args.next().as_ref().and_then(|a| a.to_str()) {
            None => Ok(Command::Clean),
            Some(other) => Err(format!("frus clean takes no `{other}`")),
        },
        Some("build") => parse_build(args),
        Some("size") => parse_size(args),
        _ => Err(format!("no command `{}`", command.to_string_lossy())),
    }
}

fn parse_emulators(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.peekable();
    let mut launch = None;
    let mut cold = false;
    let mut create = false;
    let mut name = None;
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_string(), Some(value.to_string()))
            }
            _ => (text.clone(), None),
        };
        let mut value = |flag: &str| match &inline {
            Some(value) => Ok(value.clone()),
            None => args
                .next()
                .and_then(|v| v.into_string().ok())
                .ok_or_else(|| format!("{flag} needs a value")),
        };
        match flag.as_str() {
            "--launch" => set_once(&mut launch, value("--launch")?, "--launch")?,
            "--cold" if inline.is_none() => cold = true,
            "--create" if inline.is_none() => create = true,
            "--name" => set_once(&mut name, value("--name")?, "--name")?,
            "-h" | "--help" => return Ok(Command::EmulatorsHelp),
            _ => return Err(format!("frus emulators has no option `{text}`")),
        }
    }
    match (launch, create) {
        (Some(_), true) => Err("--launch and --create: one at a time".into()),
        (Some(id), false) => Ok(Command::Emulators(emulators::Action::Launch { id, cold })),
        (None, true) => Ok(Command::Emulators(emulators::Action::Create { name })),
        (None, false) if name.is_some() || cold => {
            Err("--name goes with --create, --cold with --launch".into())
        }
        (None, false) => Ok(Command::Emulators(emulators::Action::List)),
    }
}

fn parse_doctor(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut options = doctor::Options::default();
    for arg in args {
        match arg.to_str() {
            Some("-v" | "--verbose") => options.verbose = true,
            Some("--fix") => options.fix = true,
            Some("--machine") => options.machine = true,
            Some("-h" | "--help") => return Ok(Command::DoctorHelp),
            _ => {
                return Err(format!(
                    "frus doctor has no option `{}`",
                    arg.to_string_lossy()
                ))
            }
        }
    }
    Ok(Command::Doctor(options))
}

fn parse_create(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.peekable();
    let mut dir = None;
    let mut name = None;
    let mut org = None;
    let mut frus_path = None;
    let mut git = true;
    let mut platforms = None;
    let mut deeplink = None;
    let mut description = None;

    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str() else {
            set_once(&mut dir, PathBuf::from(arg), "<directory>")?;
            continue;
        };
        // `--name=x` and `--name x` alike.
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
            _ => (text, None),
        };
        let mut value = |flag: &str| match &inline {
            Some(value) => Ok(value.clone()),
            None => args
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| format!("{flag} needs a value")),
        };
        match flag {
            "-h" | "--help" => return Ok(Command::CreateHelp),
            "--name" => set_once(&mut name, value(flag)?, flag)?,
            "--org" => set_once(&mut org, value(flag)?, flag)?,
            "--frus-path" => set_once(&mut frus_path, PathBuf::from(value(flag)?), flag)?,
            "--no-git" if inline.is_none() => git = false,
            "--platforms" => set_once(&mut platforms, platforms::parse_list(&value(flag)?)?, flag)?,
            "--deeplink" => set_once(
                &mut deeplink,
                platforms::Deeplink::parse(&value(flag)?)?,
                flag,
            )?,
            "--description" => set_once(&mut description, value(flag)?, flag)?,
            _ if flag.starts_with('-') && flag != "-" => {
                return Err(format!("frus create has no option `{text}`"));
            }
            _ => set_once(&mut dir, PathBuf::from(text), "<directory>")?,
        }
    }

    let dir = dir.ok_or("frus create needs a directory: `frus create my_app`")?;
    Ok(Command::Create(Options {
        dir,
        name,
        org: org.unwrap_or_else(|| DEFAULT_ORG.to_string()),
        frus_path,
        git,
        platforms,
        deeplink,
        description,
    }))
}

fn parse_run(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let args: Vec<OsString> = args.collect();
    let forwarded = forwarded_run_args(&args);
    let mut args = args.into_iter().peekable();
    let mut options = run::RunOptions {
        devices: Vec::new(),
        forwarded,
        mode: cargo::Mode::Debug,
        port: run::DEFAULT_PORT,
        args: Vec::new(),
        machine: false,
        hot: None,
        simulate: None,
        web: false,
    };
    let mut port = None;
    while let Some(arg) = args.next() {
        let text = arg
            .into_string()
            .map_err(|arg| format!("`{}` is not valid text", arg.to_string_lossy()))?;
        if text == "--" {
            options.args.extend(
                args.by_ref()
                    .map(|rest| rest.to_string_lossy().into_owned()),
            );
            break;
        }
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_string(), Some(value.to_string()))
            }
            _ => (text.clone(), None),
        };
        let mut value = |flag: &str| match &inline {
            Some(value) => Ok(value.clone()),
            None => args
                .next()
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| format!("{flag} needs a value")),
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(Command::RunHelp),
            "-d" | "--device" => options.devices.push(value("--device")?),
            "--release" if inline.is_none() => options.mode = cargo::Mode::Release,
            "--machine" if inline.is_none() => options.machine = true,
            "--hot" | "--no-hot" if inline.is_none() => {
                if options.hot.is_some() {
                    return Err("--hot and --no-hot: one of them, once".into());
                }
                options.hot = Some(flag == "--hot");
            }
            "--web" if inline.is_none() => options.web = true,
            "--port" => {
                let text = value(&flag)?;
                let number = text
                    .parse()
                    .map_err(|_| format!("--port takes a number, not `{text}`"))?;
                set_once(&mut port, number, "--port")?;
            }
            _ => return Err(format!("frus run has no option `{text}`")),
        }
    }
    if let Some(port) = port {
        options.port = port;
    }
    Ok(Command::Run(options))
}

/// `frus run`'s arguments without the devices and `--machine`: what each device's own run is
/// started with when there are several (see `sessions`). Everything after `--` is the
/// application's, and kept as it is.
fn forwarded_run_args(args: &[OsString]) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = args.iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                out.push(arg);
                out.extend(args.by_ref());
            }
            "-d" | "--device" => {
                args.next();
            }
            "--machine" => {}
            _ if arg.starts_with("--device=") => {}
            _ => out.push(arg),
        }
    }
    out
}

fn parse_build(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut target = None;
    let mut mode = cargo::Mode::Release;
    let mut analyze = false;
    let mut compare = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-h" | "--help" => return Ok(Command::BuildHelp),
            "--debug" => mode = cargo::Mode::Debug,
            "--analyze-size" => analyze = true,
            "--compare" => set_once(
                &mut compare,
                PathBuf::from(args.next().ok_or("--compare needs a file")?),
                "--compare",
            )?,
            flag if flag.starts_with('-') => {
                return Err(format!("frus build has no option `{flag}`"));
            }
            word => set_once(&mut target, build::Target::parse(word)?, "the target")?,
        }
    }
    let target = target.ok_or("frus build needs a target: `frus build apk`, `web` or `desktop`")?;
    if analyze || compare.is_some() {
        if mode == cargo::Mode::Debug {
            return Err("--analyze-size measures the release build: leave out --debug".into());
        }
        return Ok(Command::Size(size::Options {
            build: Some(target),
            file: None,
            compare,
        }));
    }
    Ok(Command::Build(target, mode))
}

/// `frus size <apk|web|desktop|file> [--compare old.json]`.
fn parse_size(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut what: Option<String> = None;
    let mut compare = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        match text.as_str() {
            "-h" | "--help" => return Ok(Command::SizeHelp),
            "--compare" => set_once(
                &mut compare,
                PathBuf::from(args.next().ok_or("--compare needs a file")?),
                "--compare",
            )?,
            flag if flag.starts_with('-') => {
                return Err(format!("frus size has no option `{flag}`"));
            }
            _ => set_once(&mut what, text, "what to measure")?,
        }
    }
    let what =
        what.ok_or("frus size needs a target or a file: `frus size apk`, `frus size app.wasm`")?;
    // A target's word, or a file.
    let looks_like_file = what.contains(['.', '/', '\\']);
    let (build, file) = match build::Target::parse(&what) {
        Ok(target) if !looks_like_file => (Some(target), None),
        _ => (None, Some(PathBuf::from(what))),
    };
    Ok(Command::Size(size::Options {
        build,
        file,
        compare,
    }))
}

/// A command's result, as the program's exit.
fn outcome(result: Result<ExitCode, String>) -> ExitCode {
    result.unwrap_or_else(|error| {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    })
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => format!("1 {word}"),
        n => format!("{n} {word}s"),
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, what: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{what} is given twice"));
    }
    *slot = Some(value);
    Ok(())
}

/// What `frus create` prints once the application is written: where it is, and the commands
/// that run it.
fn report(created: &Created) -> String {
    let project = &created.project;
    let dir = created.dir.display();
    if created.existing {
        return report_existing(created);
    }
    let mut out = format!(
        "Created {} in {dir} ({} files).\n",
        project.project_name,
        created.written.len()
    );
    if created.kept > 0 {
        out.push_str(&format!(
            "{} of the template's files were there already and are left as they are.\n",
            created.kept
        ));
    }
    match &created.git {
        Git::Initialised => out.push_str("Initialised a git repository.\n"),
        Git::InsideRepository => {
            out.push_str("The directory is inside a git repository already: no new one.\n");
        }
        Git::Failed(why) => out.push_str(&format!("No git repository: {why}\n")),
        Git::Skipped => {}
    }
    use platforms::AppPlatform;
    let made_for = &created.platforms;
    if made_for.as_slice() != AppPlatform::ALL {
        out.push_str(&format!(
            "Made for {}. `frus create --platforms <list> .` adds others later.\n",
            platforms::words(made_for)
        ));
    }
    let cd = if created.dir == std::path::Path::new(".") {
        String::new()
    } else {
        format!("  cd {}\n", quote(&dir.to_string()))
    };
    out.push_str(&format!("\nTo run it:\n\n{cd}"));
    let has = |platform| made_for.contains(&platform);
    if made_for.contains(&AppPlatform::host()) {
        out.push_str("  frus run                   # on this computer\n");
    }
    if has(AppPlatform::Android) {
        out.push_str("  frus run -d android        # on the connected phone\n");
    }
    if has(AppPlatform::Web) {
        out.push_str("  frus run -d web            # in a browser\n");
    }
    if made_for.contains(&AppPlatform::host()) && has(AppPlatform::Android) {
        out.push_str("  frus run -d all            # on this computer and the phone at once\n");
    }
    out.push_str(
        "\nWhat a platform needs and this machine lacks is installed on the way.\n\
         Shipping and the releases are in README.md.\n",
    );
    if has(AppPlatform::Android) {
        out.push_str(&format!(
            "The Android package is {}: settle it before the first release.\n",
            project.android_package()
        ));
    }
    if let Some(link) = &created.deeplink {
        out.push_str(&deeplink_report(
            link,
            &project.android_package(),
            has(AppPlatform::Android),
        ));
    }
    out
}

/// What `frus create --deeplink` says: how to try a link, and what the host must serve.
fn deeplink_report(link: &platforms::Deeplink, package: &str, android: bool) -> String {
    let example = link.example();
    let mut out = format!(
        "\n{example} opens the application at the location /home: a router's page, or \
         Application::open_location.\n"
    );
    if android {
        out.push_str(&format!(
            "Try it on the phone: adb shell am start -a android.intent.action.VIEW -d \"{example}\"\n"
        ));
    }
    if let Some(json) = link.assetlinks(package).filter(|_| android) {
        out.push_str(&format!(
            "\nAndroid opens these links without asking once {}/.well-known/assetlinks.json \
             says so:\n\n{json}\nThe fingerprint is your release key's: \
             keytool -list -v -keystore release.keystore\n",
            link.text()
        ));
    }
    if let Some(scheme) = link.scheme() {
        out.push_str(&format!(
            "On a Mac the .app frus builds declares {scheme}://; Windows and Linux learn it when \
             the application is installed (a registry key, a .desktop file).\n"
        ));
    }
    out
}

/// What `frus create` prints in an application that was there: what came back.
fn report_existing(created: &Created) -> String {
    let name = &created.project.project_name;
    if created.written.is_empty() && created.manifest_changes.is_empty() {
        return format!(
            "{name} has every file the template gives an application: nothing to recreate.\n"
        );
    }
    let mut out = String::new();
    if !created.written.is_empty() {
        // A platform added writes files that were never there; otherwise they came back.
        let verb = if created.manifest_changes.is_empty() {
            "Recreated"
        } else {
            "Wrote"
        };
        out.push_str(&format!(
            "{verb} {} missing file{} in {name}:\n",
            created.written.len(),
            if created.written.len() == 1 { "" } else { "s" }
        ));
        for path in &created.written {
            out.push_str(&format!("  {path}\n"));
        }
    }
    if !created.manifest_changes.is_empty() {
        out.push_str("Added:\n");
        for change in &created.manifest_changes {
            out.push_str(&format!("  {change}\n"));
        }
    }
    out.push_str(if created.manifest_changes.is_empty() {
        "Nothing that was there has been changed.\n"
    } else {
        "Nothing else that was there has been changed.\n"
    });
    if let Some(link) = &created.deeplink {
        out.push_str(&deeplink_report(
            link,
            &created.project.android_package(),
            created.platforms.contains(&platforms::AppPlatform::Android),
        ));
    }
    out
}

/// A path as a shell needs it typed.
fn quote(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || "'\"$&;()|<>".contains(c)) {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::template::FILES;

    fn args(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    fn create_options(line: &str) -> Options {
        match parse(args(line)) {
            Ok(Command::Create(options)) => options,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn commands() {
        assert_eq!(parse(args("")), Ok(Command::Help));
        assert_eq!(parse(args("help")), Ok(Command::Help));
        assert_eq!(parse(args("--help")), Ok(Command::Help));
        assert_eq!(parse(args("help create")), Ok(Command::CreateHelp));
        assert_eq!(parse(args("create --help")), Ok(Command::CreateHelp));
        assert_eq!(parse(args("create app -h")), Ok(Command::CreateHelp));
        assert_eq!(parse(args("--version")), Ok(Command::Version));
        assert_eq!(parse(args("-V")), Ok(Command::Version));
        assert!(parse(args("deploy"))
            .unwrap_err()
            .contains("no command `deploy`"));
        assert!(parse(args("help deploy"))
            .unwrap_err()
            .contains("no command `deploy`"));
        assert_eq!(parse(args("help run")), Ok(Command::RunHelp));
        assert_eq!(parse(args("build -h")), Ok(Command::BuildHelp));
        assert_eq!(parse(args("devices")), Ok(Command::Devices));
        assert_eq!(parse(args("devices --help")), Ok(Command::DevicesHelp));
    }

    fn run_options(line: &str) -> run::RunOptions {
        match parse(args(line)) {
            Ok(Command::Run(options)) => options,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn run_options_parse() {
        let plain = run_options("run");
        assert!(plain.devices.is_empty());
        assert_eq!(plain.mode, cargo::Mode::Debug);
        assert_eq!(plain.port, run::DEFAULT_PORT);
        assert!(!plain.web);
        assert_eq!(plain.hot, None, "decided by run once the device is known");
        assert_eq!(run_options("run --hot").hot, Some(true));
        assert_eq!(run_options("run --no-hot").hot, Some(false));

        let web = run_options("run -d iphone-15 --web");
        assert_eq!(web.devices, ["iphone-15"]);
        assert!(web.web);
        assert_eq!(web.simulate, None, "set by run once the device is known");

        let full = run_options("run -d edge --release --port=9000 -- /widget/button --flag");
        assert_eq!(full.devices, ["edge"]);
        assert_eq!(full.mode, cargo::Mode::Release);
        assert_eq!(full.port, 9000);
        assert_eq!(full.args, ["/widget/button", "--flag"]);
        assert!(!full.machine);
        assert!(run_options("run --machine -d windows").machine);
        assert_eq!(parse(args("daemon")), Ok(Command::Daemon));

        assert_eq!(run_options("run --device=android").devices, ["android"]);
        let error = |line| parse(args(line)).unwrap_err();
        assert!(error("run -d").contains("needs a value"));
        assert!(error("run --port x").contains("takes a number"));
        assert!(error("run --verbose").contains("no option `--verbose`"));
        assert!(error("run --hot --no-hot").contains("one of them"));
        assert!(error("run --no-hot --no-hot").contains("one of them"));
        assert!(error("run --hot=yes").contains("no option"));
    }

    /// `-d` as often as there are devices, or `all`; each device's own run gets the rest of the
    /// command line (milestone 791).
    #[test]
    fn several_devices_are_read() {
        let several = run_options("run -d windows --release -d android --port 9000 -- /home -d x");
        assert_eq!(several.devices, ["windows", "android"]);
        assert_eq!(several.args, ["/home", "-d", "x"]);
        assert_eq!(
            several.forwarded,
            ["--release", "--port", "9000", "--", "/home", "-d", "x"]
        );
        let all = run_options("run -d all --machine --device=edge --no-hot");
        assert_eq!(all.devices, ["all", "edge"]);
        assert!(all.machine);
        assert_eq!(all.forwarded, ["--no-hot"]);
    }

    #[test]
    fn build_options_parse() {
        assert_eq!(
            parse(args("build apk")),
            Ok(Command::Build(build::Target::Apk, cargo::Mode::Release))
        );
        assert_eq!(
            parse(args("build --debug web")),
            Ok(Command::Build(build::Target::Web, cargo::Mode::Debug))
        );
        let error = |line| parse(args(line)).unwrap_err();
        assert!(error("build").contains("needs a target"));
        assert!(error("build apk web").contains("given twice"));
        assert!(error("build ios").contains("not `ios`"));
    }

    #[test]
    fn size_options_parse() {
        let measure = |line| match parse(args(line)) {
            Ok(Command::Size(options)) => options,
            other => panic!("{line}: {other:?}"),
        };
        assert_eq!(
            measure("build apk --analyze-size"),
            size::Options {
                build: Some(build::Target::Apk),
                file: None,
                compare: None,
            }
        );
        let compared = measure("build --analyze-size web --compare old.json");
        assert_eq!(compared.build, Some(build::Target::Web));
        assert_eq!(compared.compare, Some(PathBuf::from("old.json")));
        assert_eq!(measure("size desktop").build, Some(build::Target::Desktop));
        let file = measure("size target/x/app.wasm --compare a.json");
        assert_eq!(file.build, None);
        assert_eq!(file.file, Some(PathBuf::from("target/x/app.wasm")));
        assert_eq!(parse(args("size --help")), Ok(Command::SizeHelp));
        assert_eq!(parse(args("help size")), Ok(Command::SizeHelp));
        let error = |line| parse(args(line)).unwrap_err();
        assert!(error("size").contains("needs a target or a file"));
        assert!(error("build apk --analyze-size --debug").contains("release"));
        assert!(error("size apk --what").contains("no option"));
    }

    #[test]
    fn emulators_and_clean_are_read() {
        use emulators::Action;
        assert_eq!(
            parse(args("emulators")),
            Ok(Command::Emulators(Action::List))
        );
        assert_eq!(
            parse(args("emulators --launch pixel_7 --cold")),
            Ok(Command::Emulators(Action::Launch {
                id: "pixel_7".into(),
                cold: true
            }))
        );
        assert_eq!(
            parse(args("emulators --create --name=mine")),
            Ok(Command::Emulators(Action::Create {
                name: Some("mine".into())
            }))
        );
        assert!(parse(args("emulators --launch a --create")).is_err());
        assert!(parse(args("emulators --name x")).is_err());
        assert_eq!(parse(args("emulators --help")), Ok(Command::EmulatorsHelp));
        assert_eq!(parse(args("help emulators")), Ok(Command::EmulatorsHelp));
        assert_eq!(parse(args("clean")), Ok(Command::Clean));
        assert_eq!(
            parse(args("test")),
            Ok(Command::Test {
                filter: None,
                verbose: false
            })
        );
        assert_eq!(
            parse(args("test adds -v")),
            Ok(Command::Test {
                filter: Some("adds".into()),
                verbose: true
            })
        );
        assert!(parse(args("test --what")).is_err());
        assert_eq!(parse(args("analyze")), Ok(Command::Analyze { fix: false }));
        assert_eq!(
            parse(args("analyze --fix")),
            Ok(Command::Analyze { fix: true })
        );
        assert!(parse(args("analyze --all")).is_err());
        assert!(parse(args("clean --all")).is_err());
    }

    #[test]
    fn create_defaults() {
        let options = create_options("create my_app");
        assert_eq!(
            options,
            Options {
                dir: PathBuf::from("my_app"),
                name: None,
                org: "com.example".into(),
                frus_path: None,
                git: true,
                platforms: None,
                deeplink: None,
                description: None,
            }
        );
    }

    #[test]
    fn create_options_in_both_spellings_and_any_order() {
        let options = create_options(
            "create --org com.acme apps/x --name=my-app --frus-path=../frus --no-git",
        );
        assert_eq!(options.dir, PathBuf::from("apps/x"));
        assert_eq!(options.name.as_deref(), Some("my-app"));
        assert_eq!(options.org, "com.acme");
        assert_eq!(options.frus_path, Some(PathBuf::from("../frus")));
        assert!(!options.git);
    }

    #[test]
    fn create_mistakes() {
        let error = |line| parse(args(line)).unwrap_err();
        assert!(error("create").contains("needs a directory"));
        assert!(error("create a b").contains("<directory> is given twice"));
        assert!(error("create a --name").contains("--name needs a value"));
        assert!(error("create a --org x --org y").contains("--org is given twice"));
        assert!(error("create a --force").contains("no option `--force`"));
        assert!(error("create a --no-git=yes").contains("no option `--no-git=yes`"));
    }

    #[test]
    fn the_report_says_how_to_run_it() {
        let created = Created {
            project: create::Project::new("my_app", "com.acme").unwrap(),
            dir: PathBuf::from("my apps/my_app"),
            written: vec!["Cargo.toml".to_string(); 14],
            kept: 0,
            existing: false,
            git: Git::Initialised,
            platforms: platforms::AppPlatform::ALL.to_vec(),
            deeplink: None,
            manifest_changes: Vec::new(),
        };
        let out = report(&created);
        assert!(
            out.starts_with("Created my_app in my apps/my_app (14 files).\n"),
            "{out}"
        );
        assert!(out.contains("Initialised a git repository."));
        assert!(
            out.contains("  cd \"my apps/my_app\"\n  frus run "),
            "{out}"
        );
        assert!(out.contains("com.acme.my_app"));

        let here = Created {
            dir: PathBuf::from("."),
            git: Git::Skipped,
            ..created
        };
        let out = report(&here);
        assert!(!out.contains("cd "), "{out}");
        assert!(!out.contains("git"), "{out}");

        let restored = Created {
            written: vec!["web/index.html".to_string()],
            kept: 13,
            existing: true,
            ..here.clone()
        };
        assert_eq!(
            report(&restored),
            "Recreated 1 missing file in my_app:\n  web/index.html\n\
             Nothing that was there has been changed.\n"
        );
        let whole = Created {
            written: Vec::new(),
            ..restored.clone()
        };
        assert!(report(&whole).contains("nothing to recreate"));

        // A platform added (milestone 791): its files, and what the manifest gained.
        let added = Created {
            manifest_changes: vec!["Cargo.toml: the web section".into()],
            ..restored
        };
        assert_eq!(
            report(&added),
            "Wrote 1 missing file in my_app:\n  web/index.html\n\
             Added:\n  Cargo.toml: the web section\n\
             Nothing else that was there has been changed.\n"
        );
    }

    // --- The template and the crate stay in step ---

    fn template_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("template")
    }

    fn template_text(path: &str) -> String {
        let file = FILES.iter().find(|file| file.path == path).unwrap();
        std::str::from_utf8(file.bytes)
            .unwrap()
            .replace("\r\n", "\n")
    }

    /// Every file in `template/` is built into the program, and nothing else is.
    #[test]
    fn every_template_file_is_built_in() {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let relative = path.strip_prefix(root).unwrap();
                    let parts: Vec<_> = relative
                        .components()
                        .map(|c| c.as_os_str().to_str().unwrap())
                        .collect();
                    out.push(parts.join("/"));
                }
            }
        }
        let mut on_disk = Vec::new();
        walk(&template_dir(), &template_dir(), &mut on_disk);
        on_disk.sort();
        let built_in: Vec<String> = FILES.iter().map(|file| file.path.to_string()).collect();
        assert_eq!(built_in, on_disk, "template.rs's list and template/ differ");
    }

    /// The files `frus create` copies untouched are the ones `cargo generate` is told to.
    #[test]
    fn the_verbatim_files_are_the_cargo_generate_excludes() {
        let config = template_text("cargo-generate.toml");
        let exclude = config
            .lines()
            .find(|line| line.starts_with("exclude = "))
            .unwrap();
        assert_eq!(exclude, r#"exclude = ["*.png", ".github/workflows/*.yml"]"#);
        let verbatim: Vec<_> = FILES
            .iter()
            .map(|f| f.path)
            .filter(|path| create::is_verbatim(path))
            .collect();
        assert_eq!(verbatim.len(), 7, "{verbatim:?}");
    }

    /// A new application depends on the frus this tool was released with, and asks for the
    /// compiler frus asks for.
    #[test]
    fn the_template_tracks_this_release() {
        let manifest = template_text("Cargo.toml.liquid");
        let version = env!("CARGO_PKG_VERSION");
        let major_minor: Vec<_> = version.split('.').take(2).collect();
        let requirement = format!("frus = \"{}\"", major_minor.join("."));
        assert!(
            manifest.lines().any(|line| line == requirement),
            "the template should depend on {requirement}"
        );
        let rust_version = format!("rust-version = \"{}\"", env!("CARGO_PKG_RUST_VERSION"));
        assert!(
            manifest.lines().any(|line| line == rust_version),
            "the template should ask for {rust_version}"
        );
    }
}
