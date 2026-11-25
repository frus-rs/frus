// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! What the workbench shows, and how it changes.
//!
//! Everything that happens — a key, a click, a line from a run — is a [`Msg`] handed to
//! [`App::update`], which changes the state and answers with the [`Effect`]s to carry out:
//! a run to start, a command to send, a build to begin. Nothing here starts a process or
//! touches the terminal, so the whole of the workbench's behaviour is tested without either.

use std::collections::BTreeSet;

use crate::view::{self, Target};

/// The most lines a pane keeps; the oldest go first.
const CAPACITY: usize = 10_000;

/// The tabs, in their order on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// Devices, the runs on them and their log.
    Run,
    /// `frus build`.
    Build,
    /// `frus doctor`, and `--fix`.
    Doctor,
}

impl Tab {
    pub(crate) const ALL: [Tab; 3] = [Tab::Run, Tab::Build, Tab::Doctor];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Tab::Run => "Run",
            Tab::Build => "Build",
            Tab::Doctor => "Doctor",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// The panel of the Run tab the keys go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Devices,
    Runs,
    Log,
}

/// A key or a mouse action, as the workbench reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Char(char),
    /// A letter with Ctrl held.
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    BackTab,
    Enter,
    Esc,
    /// The left button, pressed at a column and a row.
    Click(u16, u16),
    WheelUp(u16, u16),
    WheelDown(u16, u16),
}

/// A device, as `frus daemon` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub category: String,
    pub detail: String,
    pub emulator: bool,
    /// Why it cannot be run on, when it cannot (a phone not authorised yet).
    pub unavailable: Option<String>,
}

/// An emulator that can be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emulator {
    pub id: String,
    pub name: String,
    pub platform: String,
}

/// Where a run is.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStatus {
    /// Building and installing: the last thing the tool said.
    Building(String),
    /// The application is up.
    Running { hot: bool },
    /// A reload (`r`) or a restart (`R`) was asked and is not done yet.
    Reloading { full: bool },
    /// The run ended: why, when it failed.
    Stopped { error: Option<String> },
}

/// One `frus run --machine` process: an application on one device.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The workbench's own number for it, which its messages carry.
    pub key: usize,
    pub device_id: String,
    pub device_name: String,
    pub status: RunStatus,
    /// Whether the code is patched in as files are saved (`app.started`'s `hotReload`).
    pub hot: bool,
}

impl Run {
    /// Whether the process may still be running.
    pub fn is_live(&self) -> bool {
        !matches!(self.status, RunStatus::Stopped { .. })
    }
}

/// What a line of a pane is, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// The application's own log.
    App,
    /// The application's error output.
    AppError,
    /// What the tool says.
    Tool,
    /// A failure the tool reports.
    ToolError,
    /// The output of the programs the tool runs: cargo, cargo-apk, wasm-bindgen.
    Output,
    /// A good result.
    Good,
}

/// A line of a pane, and the device it comes from when several run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub source: Option<String>,
    pub text: String,
    pub kind: LineKind,
}

/// A scrolling list of lines.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pane {
    pub lines: Vec<Line>,
    /// How many lines up from the end it is scrolled: 0 follows what comes.
    pub scroll: usize,
}

impl Pane {
    pub(crate) fn push(&mut self, source: Option<&str>, text: &str, kind: LineKind) {
        if self.lines.len() >= CAPACITY {
            self.lines.drain(..CAPACITY / 10);
        }
        self.lines.push(Line {
            source: source.map(str::to_string),
            text: text.to_string(),
            kind,
        });
        // Scrolled back, the view stays where it is as lines arrive.
        if self.scroll > 0 {
            self.scroll += 1;
        }
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    /// Scrolls up (`by` > 0) or down, `height` lines being shown.
    fn scroll(&mut self, by: isize, height: usize) {
        let most = self.lines.len().saturating_sub(height);
        self.scroll = self.scroll.saturating_add_signed(by).min(most);
    }
}

/// Where a command run in the background is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// Never started.
    Idle,
    Running,
    /// Ended, with its exit code when it had one.
    Done {
        code: Option<i32>,
    },
}

/// The commands the workbench runs to the end: a build, the doctor, its fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Build,
    Doctor,
    Fix,
}

/// One part of `frus doctor`'s report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    /// `ready`, `partial` or `missing`.
    pub status: String,
    pub summary: String,
    /// Each finding, with whether it is good (`None`: only information).
    pub findings: Vec<(Option<bool>, String)>,
}

/// What a run reports, read from its JSON lines and its error output.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
    Start {
        device_name: String,
    },
    /// DevTools are served for the run at this address.
    DevTools(String),
    Progress {
        message: String,
        error: bool,
    },
    Started {
        hot: bool,
    },
    Log {
        text: String,
        error: bool,
    },
    Restarted {
        full: bool,
        seconds: f64,
    },
    Stop {
        error: Option<String>,
    },
    /// A line of the run's error output: the build's.
    Output(String),
    /// The process ended, with its exit code when it had one.
    Exited(Option<i32>),
}

/// What happens, for [`App::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    Input(Input),
    /// The terminal's new size.
    Resize(u16, u16),
    Devices(Vec<Device>),
    DeviceAdded(Device),
    DeviceRemoved(String),
    Emulators(Vec<Emulator>),
    /// What the daemon says: `daemon.logMessage`, `daemon.showMessage`, an error answer.
    DaemonMessage {
        text: String,
        error: bool,
    },
    /// `frus daemon` ended or could not start.
    DaemonGone(String),
    Run {
        key: usize,
        event: RunEvent,
    },
    JobLine {
        job: JobKind,
        text: String,
        error: bool,
    },
    JobDone {
        job: JobKind,
        code: Option<i32>,
    },
}

/// A command for a run, as `frus run --machine` takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCommand {
    /// `r`: the code patched in, or a restart where it cannot be.
    Reload,
    /// `R`: a restart, every state afresh.
    Restart,
    /// `q`: the run ends.
    Stop,
}

/// What [`App::update`] asks to be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `frus run --machine -d <device>`, to be known as `key`.
    StartRun {
        key: usize,
        device: String,
    },
    Send {
        key: usize,
        command: RunCommand,
    },
    /// `emulator.launch` to the daemon.
    LaunchEmulator(String),
    /// `frus build <target>`.
    Build(String),
    /// `frus doctor --machine`.
    Doctor,
    /// `frus doctor --fix`.
    Fix,
    /// Ends a job still running.
    Cancel(JobKind),
    /// Leave: stop every run, end the daemon, put the terminal back.
    Quit,
}

/// What `frus build` makes, with what the target list says of each.
pub const BUILD_TARGETS: [(&str, &str); 5] = [
    ("desktop", "this computer's program, in release"),
    ("web", "the page and its .wasm, ready to serve"),
    ("apk", "an Android package to install"),
    ("appbundle", "an Android App Bundle for the Play Store"),
    ("installer", "this computer's installer"),
];

/// An entry of the Devices panel, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Device(usize),
    Emulator(usize),
}

/// The whole state of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub tab: Tab,
    pub focus: Focus,
    /// The application `frus` was started in: its package's name.
    pub project: Option<String>,
    pub devices: Vec<Device>,
    pub emulators: Vec<Emulator>,
    /// Whether the daemon answered with the devices yet.
    pub devices_known: bool,
    pub device_cursor: usize,
    /// The devices picked to run on together (Space), by id.
    pub marked: BTreeSet<String>,
    pub runs: Vec<Run>,
    pub run_cursor: usize,
    pub log: Pane,
    pub build_cursor: usize,
    pub build: Pane,
    pub build_state: JobState,
    pub build_target: Option<String>,
    pub checks: Vec<Check>,
    pub doctor: Pane,
    pub doctor_state: JobState,
    pub fix_state: JobState,
    /// The last thing worth saying, at the bottom of the screen.
    pub notice: Option<(String, bool)>,
    /// The terminal's size: what clicks are measured against.
    pub size: (u16, u16),
    pub quitting: bool,
    next_key: usize,
}

impl App {
    pub fn new(project: Option<String>) -> Self {
        App {
            tab: Tab::Run,
            focus: Focus::Devices,
            project,
            devices: Vec::new(),
            emulators: Vec::new(),
            devices_known: false,
            device_cursor: 0,
            marked: BTreeSet::new(),
            runs: Vec::new(),
            run_cursor: 0,
            log: Pane::default(),
            build_cursor: 0,
            build: Pane::default(),
            build_state: JobState::Idle,
            build_target: None,
            checks: Vec::new(),
            doctor: Pane::default(),
            doctor_state: JobState::Idle,
            fix_state: JobState::Idle,
            notice: None,
            size: (80, 24),
            quitting: false,
            next_key: 1,
        }
    }

    /// The Devices panel's entries: the devices, then the emulators that can be started.
    pub fn rows(&self) -> Vec<Row> {
        (0..self.devices.len())
            .map(Row::Device)
            .chain((0..self.emulators.len()).map(Row::Emulator))
            .collect()
    }

    /// The run on a device that has not ended, if there is one.
    pub fn live_run_on(&self, device: &str) -> Option<&Run> {
        self.runs
            .iter()
            .find(|r| r.device_id == device && r.is_live())
    }

    /// Whether the log names the device of each line: when more than one device ran.
    pub fn several_devices(&self) -> bool {
        let mut ids = self.runs.iter().map(|r| r.device_id.as_str());
        ids.next()
            .is_some_and(|first| ids.any(|other| other != first))
    }

    fn say(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), false));
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), true));
    }

    /// Takes what happened in, and answers with what to do.
    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        match msg {
            Msg::Input(input) => return self.input(input),
            Msg::Resize(width, height) => self.size = (width, height),
            Msg::Devices(devices) => {
                let selected = self.selected_id();
                self.devices = devices;
                self.devices_known = true;
                self.select(selected);
            }
            Msg::DeviceAdded(device) => {
                let selected = self.selected_id();
                // The daemon's first look reports every device as added, and a device that
                // changes (a phone authorised) comes back as added: it keeps its place, and
                // only a new device is announced.
                match self.devices.iter_mut().find(|d| d.id == device.id) {
                    Some(known) => *known = device,
                    None => {
                        self.say(format!("{} connected", device.name));
                        self.devices.push(device);
                    }
                }
                self.devices_known = true;
                self.select(selected);
            }
            Msg::DeviceRemoved(id) => {
                let selected = self.selected_id();
                // A device that changes (a phone authorised) is removed and added again at
                // once: only the one that stays away is said to have gone.
                self.devices.retain(|d| d.id != id);
                self.marked.remove(&id);
                self.select(selected);
            }
            Msg::Emulators(emulators) => {
                let selected = self.selected_id();
                self.emulators = emulators;
                self.select(selected);
            }
            Msg::DaemonMessage { text, error } => {
                if error {
                    self.warn(text);
                } else {
                    self.say(text);
                }
            }
            Msg::DaemonGone(why) => {
                self.devices_known = true;
                self.warn(format!("the device list stopped: {why}"));
            }
            Msg::Run { key, event } => self.run_event(key, event),
            Msg::JobLine { job, text, error } => self.job_line(job, &text, error),
            Msg::JobDone { job, code } => return self.job_done(job, code),
        }
        Vec::new()
    }

    /// The id of the entry under the cursor.
    fn selected_id(&self) -> Option<String> {
        match self.rows().get(self.device_cursor)? {
            Row::Device(i) => Some(self.devices[*i].id.clone()),
            Row::Emulator(i) => Some(format!("emulator:{}", self.emulators[*i].id)),
        }
    }

    /// Puts the cursor back on the entry it was on, wherever the list moved it: a list that
    /// changes under the cursor must never make `Enter` run somewhere else.
    fn select(&mut self, id: Option<String>) {
        let rows = self.rows();
        let found = id.and_then(|id| {
            rows.iter().position(|row| match row {
                Row::Device(i) => self.devices[*i].id == id,
                Row::Emulator(i) => id == format!("emulator:{}", self.emulators[*i].id),
            })
        });
        self.device_cursor = found.unwrap_or(self.device_cursor.min(rows.len().saturating_sub(1)));
    }

    fn input(&mut self, input: Input) -> Vec<Effect> {
        // What every tab understands.
        match input {
            Input::Char('q') | Input::Ctrl('c') => {
                self.quitting = true;
                return vec![Effect::Quit];
            }
            Input::Char(c @ '1'..='3') => {
                return self.switch(Tab::ALL[c as usize - '1' as usize]);
            }
            Input::Left => {
                return self.switch(Tab::ALL[(self.tab.index() + 2) % 3]);
            }
            Input::Right => {
                return self.switch(Tab::ALL[(self.tab.index() + 1) % 3]);
            }
            Input::Click(column, row)
            | Input::WheelUp(column, row)
            | Input::WheelDown(column, row) => {
                return self.mouse(input, view::hit(self, column, row));
            }
            _ => {}
        }
        match self.tab {
            Tab::Run => self.run_input(input),
            Tab::Build => self.build_input(input),
            Tab::Doctor => self.doctor_input(input),
        }
    }

    fn switch(&mut self, tab: Tab) -> Vec<Effect> {
        self.tab = tab;
        // The doctor looks the first time its tab is opened: what it shows is its answer.
        if tab == Tab::Doctor && self.doctor_state == JobState::Idle {
            return self.start_doctor();
        }
        Vec::new()
    }

    fn start_doctor(&mut self) -> Vec<Effect> {
        if self.doctor_state == JobState::Running || self.fix_state == JobState::Running {
            return Vec::new();
        }
        self.doctor_state = JobState::Running;
        self.doctor.clear();
        vec![Effect::Doctor]
    }

    fn mouse(&mut self, input: Input, target: Option<Target>) -> Vec<Effect> {
        let Some(target) = target else {
            return Vec::new();
        };
        let wheel = match input {
            Input::WheelUp(..) => Some(Input::Up),
            Input::WheelDown(..) => Some(Input::Down),
            _ => None,
        };
        match (target, wheel) {
            (Target::Tab(tab), None) => self.switch(tab),
            (Target::Button(key), None) => self.input(key),
            (Target::Mark(index), None) => {
                self.focus = Focus::Devices;
                self.device_cursor = index;
                self.input(Input::Char(' '))
            }
            (Target::DeviceRow(index), None) => {
                self.focus = Focus::Devices;
                self.device_cursor = index;
                Vec::new()
            }
            (Target::RunRow(index), None) => {
                self.focus = Focus::Runs;
                self.run_cursor = index;
                Vec::new()
            }
            (Target::BuildRow(index), None) => {
                self.build_cursor = index;
                Vec::new()
            }
            (Target::Pane, None) => {
                if self.tab == Tab::Run {
                    self.focus = Focus::Log;
                }
                Vec::new()
            }
            // The wheel scrolls a pane three lines at a time, and moves through a list.
            (Target::Pane, Some(direction)) => {
                let by = if direction == Input::Up { 3 } else { -3 };
                self.scroll_pane(by);
                Vec::new()
            }
            (Target::DeviceRow(_) | Target::Mark(_), Some(direction)) => {
                self.focus = Focus::Devices;
                self.run_input(direction)
            }
            (Target::RunRow(_), Some(direction)) => {
                self.focus = Focus::Runs;
                self.run_input(direction)
            }
            (Target::BuildRow(_), Some(direction)) => self.build_input(direction),
            (_, Some(_)) => Vec::new(),
        }
    }

    /// The pane of the tab shown, and how many lines of it are seen.
    fn pane(&mut self) -> (&mut Pane, usize) {
        let height = view::pane_height(self);
        let pane = match self.tab {
            Tab::Run => &mut self.log,
            Tab::Build => &mut self.build,
            Tab::Doctor => &mut self.doctor,
        };
        (pane, height)
    }

    fn scroll_pane(&mut self, by: isize) {
        let (pane, height) = self.pane();
        pane.scroll(by, height);
    }

    /// Page Up and Down, Home and End: the pane of the tab, whatever has the focus.
    fn pane_keys(&mut self, input: Input) -> bool {
        let (pane, height) = self.pane();
        let page = height.max(2) as isize - 1;
        match input {
            Input::PageUp => pane.scroll(page, height),
            Input::PageDown => pane.scroll(-page, height),
            Input::Home => pane.scroll(isize::MAX, height),
            Input::End => pane.scroll = 0,
            _ => return false,
        }
        true
    }

    fn run_input(&mut self, input: Input) -> Vec<Effect> {
        if self.pane_keys(input) {
            return Vec::new();
        }
        let rows = self.rows().len();
        match input {
            Input::Tab => {
                self.focus = match self.focus {
                    Focus::Devices => Focus::Runs,
                    Focus::Runs => Focus::Log,
                    Focus::Log => Focus::Devices,
                }
            }
            Input::BackTab => {
                self.focus = match self.focus {
                    Focus::Devices => Focus::Log,
                    Focus::Runs => Focus::Devices,
                    Focus::Log => Focus::Runs,
                }
            }
            Input::Up | Input::Char('k') => match self.focus {
                Focus::Devices => self.device_cursor = self.device_cursor.saturating_sub(1),
                Focus::Runs => self.run_cursor = self.run_cursor.saturating_sub(1),
                Focus::Log => self.scroll_pane(1),
            },
            Input::Down | Input::Char('j') => match self.focus {
                Focus::Devices => {
                    self.device_cursor = (self.device_cursor + 1).min(rows.saturating_sub(1))
                }
                Focus::Runs => {
                    self.run_cursor = (self.run_cursor + 1).min(self.runs.len().saturating_sub(1))
                }
                Focus::Log => self.scroll_pane(-1),
            },
            Input::Char(' ') => self.toggle_mark(),
            Input::Enter => return self.enter(),
            Input::Char('r') => return self.command(RunCommand::Reload),
            Input::Char('R') => return self.command(RunCommand::Restart),
            Input::Char('s') => return self.command(RunCommand::Stop),
            Input::Char('c') => {
                self.log.clear();
            }
            _ => {}
        }
        Vec::new()
    }

    fn toggle_mark(&mut self) {
        if let Some(Row::Device(index)) = self.rows().get(self.device_cursor).copied() {
            let id = self.devices[index].id.clone();
            if !self.marked.remove(&id) {
                self.marked.insert(id);
            }
        }
    }

    /// Enter: on a device, run there (on every marked device, when some are); on an emulator,
    /// start it; on an ended run, run again on its device.
    fn enter(&mut self) -> Vec<Effect> {
        match self.focus {
            Focus::Runs => {
                let Some(run) = self.runs.get(self.run_cursor) else {
                    return Vec::new();
                };
                if run.is_live() {
                    self.say(format!(
                        "{} is running: r reloads it, s stops it",
                        run.device_name
                    ));
                    return Vec::new();
                }
                let id = run.device_id.clone();
                self.start_runs(vec![id])
            }
            Focus::Devices | Focus::Log => match self.rows().get(self.device_cursor).copied() {
                Some(Row::Emulator(index)) => {
                    let emulator = &self.emulators[index];
                    let (id, name) = (emulator.id.clone(), emulator.name.clone());
                    self.say(format!(
                        "Starting the emulator {name}: it is listed as a device once up"
                    ));
                    vec![Effect::LaunchEmulator(id)]
                }
                Some(Row::Device(index)) => {
                    let wanted: Vec<String> = if self.marked.is_empty() {
                        vec![self.devices[index].id.clone()]
                    } else {
                        // In the panel's order, not the order they were marked in.
                        self.devices
                            .iter()
                            .filter(|d| self.marked.contains(&d.id))
                            .map(|d| d.id.clone())
                            .collect()
                    };
                    self.start_runs(wanted)
                }
                None => {
                    self.warn("No device to run on yet");
                    Vec::new()
                }
            },
        }
    }

    fn start_runs(&mut self, ids: Vec<String>) -> Vec<Effect> {
        let mut effects = Vec::new();
        for id in ids {
            let Some(device) = self.devices.iter().find(|d| d.id == id).cloned() else {
                continue;
            };
            if let Some(reason) = &device.unavailable {
                self.warn(format!("{} cannot be run on: {reason}", device.name));
                continue;
            }
            if self.live_run_on(&id).is_some() {
                self.say(format!(
                    "Already running on {}: r reloads it, R restarts it",
                    device.name
                ));
                continue;
            }
            let key = self.next_key;
            self.next_key += 1;
            self.runs.push(Run {
                key,
                device_id: id.clone(),
                device_name: device.name.clone(),
                status: RunStatus::Building("starting".into()),
                hot: false,
            });
            self.run_cursor = self.runs.len() - 1;
            self.say(format!("Running on {}", device.name));
            effects.push(Effect::StartRun { key, device: id });
        }
        self.marked.clear();
        effects
    }

    /// `r`, `R` or `s`: to the run picked in the Runs panel when it has the focus, otherwise
    /// to every run — the way `frus run` with several devices sends each key to all.
    fn command(&mut self, command: RunCommand) -> Vec<Effect> {
        let picked: Vec<usize> = if self.focus == Focus::Runs {
            self.runs
                .get(self.run_cursor)
                .into_iter()
                .map(|r| r.key)
                .collect()
        } else {
            self.runs.iter().map(|r| r.key).collect()
        };
        let mut effects = Vec::new();
        let mut patching = false;
        for run in self.runs.iter_mut().filter(|r| picked.contains(&r.key)) {
            // Up, or waiting for a restart that has not reported yet: asking again must stay
            // possible.
            let ready = matches!(
                run.status,
                RunStatus::Running { .. } | RunStatus::Reloading { .. }
            );
            match command {
                // A reload asks for an application that is up; a stop ends any run, a build too.
                RunCommand::Reload | RunCommand::Restart if !ready => continue,
                // With hot reload, a saved file is patched in by itself: `r` has nothing to
                // send, as in `frus run`.
                RunCommand::Reload if run.hot => {
                    patching = true;
                    continue;
                }
                RunCommand::Stop if !run.is_live() => continue,
                RunCommand::Reload | RunCommand::Restart => {
                    run.status = RunStatus::Reloading {
                        full: command == RunCommand::Restart,
                    }
                }
                RunCommand::Stop => {}
            }
            effects.push(Effect::Send {
                key: run.key,
                command,
            });
        }
        if patching && effects.is_empty() {
            self.say(
                "Hot reload is on: save a file and the change is patched in, its state kept; \
                 R restarts",
            );
        } else if effects.is_empty() {
            let what = match command {
                RunCommand::Stop => "Nothing to stop",
                _ if self.runs.iter().any(Run::is_live) => {
                    "Nothing to reload yet: the application is not up"
                }
                _ => "Nothing is running: Enter runs the application on the selected device",
            };
            self.say(what);
        }
        effects
    }

    fn run_event(&mut self, key: usize, event: RunEvent) {
        let several = self.several_devices();
        let Some(run) = self.runs.iter_mut().find(|r| r.key == key) else {
            return;
        };
        let source = several.then(|| run.device_name.clone());
        let source = source.as_deref();
        match event {
            RunEvent::Start { device_name } => {
                if !device_name.is_empty() {
                    run.device_name = device_name;
                }
            }
            RunEvent::DevTools(uri) => {
                self.log
                    .push(source, &format!("DevTools: {uri}"), LineKind::Tool);
            }
            RunEvent::Progress { message, error } => {
                if let RunStatus::Building(status) = &mut run.status {
                    if let Some(first) = message.lines().find(|l| !l.trim().is_empty()) {
                        *status = first.trim().to_string();
                    }
                }
                let kind = if error {
                    LineKind::ToolError
                } else {
                    LineKind::Tool
                };
                for line in message.lines() {
                    self.log.push(source, line, kind);
                }
            }
            RunEvent::Output(text) => {
                if let RunStatus::Building(status) = &mut run.status {
                    if !text.trim().is_empty() {
                        *status = text.trim().to_string();
                    }
                }
                self.log.push(source, &text, LineKind::Output);
            }
            RunEvent::Started { hot } => {
                run.hot = hot;
                run.status = RunStatus::Running { hot };
                let text = if hot {
                    "The application is up: save a file and the change is patched in; R restarts it"
                } else {
                    "The application is up: r and R restart it"
                };
                self.log.push(source, text, LineKind::Good);
            }
            RunEvent::Log { text, error } => {
                let kind = if error {
                    LineKind::AppError
                } else {
                    LineKind::App
                };
                self.log.push(source, &text, kind);
            }
            RunEvent::Restarted { full, seconds } => {
                run.status = RunStatus::Running { hot: run.hot };
                let what = match (full, run.hot) {
                    (true, _) => "Restarted",
                    // A save, patched into the running application.
                    (false, true) => "Patched",
                    (false, false) => "Reloaded",
                };
                self.log
                    .push(source, &format!("{what} in {seconds:.1} s"), LineKind::Good);
            }
            RunEvent::Stop { error } => {
                let text = match &error {
                    Some(error) => format!("The run ended: {error}"),
                    None => "The run ended".to_string(),
                };
                let kind = if error.is_some() {
                    LineKind::ToolError
                } else {
                    LineKind::Tool
                };
                run.status = RunStatus::Stopped { error };
                self.log.push(source, &text, kind);
            }
            RunEvent::Exited(code) => {
                // `app.stop` said it already, unless the process ended without a word.
                if run.is_live() {
                    let error = match code {
                        Some(0) => None,
                        Some(code) => Some(format!("frus run ended with exit code {code}")),
                        None => Some("frus run was ended".to_string()),
                    };
                    if let Some(error) = &error {
                        self.log.push(source, error, LineKind::ToolError);
                    }
                    run.status = RunStatus::Stopped { error };
                }
            }
        }
    }

    fn build_input(&mut self, input: Input) -> Vec<Effect> {
        if self.pane_keys(input) {
            return Vec::new();
        }
        match input {
            Input::Up | Input::Char('k') => self.build_cursor = self.build_cursor.saturating_sub(1),
            Input::Down | Input::Char('j') => {
                self.build_cursor = (self.build_cursor + 1).min(BUILD_TARGETS.len() - 1)
            }
            Input::Enter | Input::Char('b') => {
                if self.build_state == JobState::Running {
                    self.say("A build is running: c cancels it");
                    return Vec::new();
                }
                let target = BUILD_TARGETS[self.build_cursor].0.to_string();
                self.build.clear();
                self.build
                    .push(None, &format!("frus build {target}"), LineKind::Tool);
                self.build_state = JobState::Running;
                self.build_target = Some(target.clone());
                return vec![Effect::Build(target)];
            }
            Input::Char('c') if self.build_state == JobState::Running => {
                return vec![Effect::Cancel(JobKind::Build)];
            }
            _ => {}
        }
        Vec::new()
    }

    fn doctor_input(&mut self, input: Input) -> Vec<Effect> {
        if self.pane_keys(input) {
            return Vec::new();
        }
        match input {
            Input::Up | Input::Char('k') => self.scroll_pane(1),
            Input::Down | Input::Char('j') => self.scroll_pane(-1),
            Input::Enter => return self.start_doctor(),
            Input::Char('f') => {
                if self.fix_state == JobState::Running || self.doctor_state == JobState::Running {
                    self.say("The doctor is busy: wait for it to finish");
                    return Vec::new();
                }
                self.fix_state = JobState::Running;
                self.doctor.push(None, "", LineKind::Tool);
                self.doctor.push(None, "frus doctor --fix", LineKind::Tool);
                return vec![Effect::Fix];
            }
            Input::Char('c') if self.fix_state == JobState::Running => {
                return vec![Effect::Cancel(JobKind::Fix)];
            }
            _ => {}
        }
        Vec::new()
    }

    fn job_line(&mut self, job: JobKind, text: &str, error: bool) {
        match job {
            JobKind::Build => {
                let kind = if error {
                    LineKind::Output
                } else {
                    LineKind::Tool
                };
                self.build.push(None, text, kind);
            }
            JobKind::Doctor => {
                // Its answer is one line of JSON: the checks.
                if let Some(checks) = (!error).then(|| parse_checks(text)).flatten() {
                    self.checks = checks;
                    self.show_checks();
                } else if !text.trim().is_empty() {
                    self.doctor.push(None, text, LineKind::Output);
                }
            }
            JobKind::Fix => self.doctor.push(None, text, LineKind::Output),
        }
    }

    fn show_checks(&mut self) {
        let mut pane = Pane::default();
        for check in &self.checks {
            let (mark, kind) = match check.status.as_str() {
                "ready" => ("[✓]", LineKind::Good),
                "partial" => ("[!]", LineKind::ToolError),
                _ => ("[✗]", LineKind::ToolError),
            };
            let text = if check.summary.is_empty() {
                format!("{mark} {}", check.name)
            } else {
                format!("{mark} {} ({})", check.name, check.summary)
            };
            pane.push(None, &text, kind);
            for (ok, finding) in &check.findings {
                let (mark, kind) = match ok {
                    Some(true) => ("•", LineKind::Tool),
                    Some(false) => ("✗", LineKind::ToolError),
                    None => ("·", LineKind::Output),
                };
                for (i, line) in finding.lines().enumerate() {
                    let mark = if i == 0 { mark } else { " " };
                    pane.push(None, &format!("    {mark} {line}"), kind);
                }
            }
        }
        self.doctor = pane;
    }

    fn job_done(&mut self, job: JobKind, code: Option<i32>) -> Vec<Effect> {
        match job {
            JobKind::Build => {
                self.build_state = JobState::Done { code };
                let target = self.build_target.clone().unwrap_or_default();
                if code == Some(0) {
                    self.build
                        .push(None, &format!("frus build {target}: done"), LineKind::Good);
                    self.say(format!("frus build {target}: done"));
                } else {
                    let why = match code {
                        Some(code) => format!("failed (exit code {code})"),
                        None => "stopped".to_string(),
                    };
                    self.build.push(
                        None,
                        &format!("frus build {target}: {why}"),
                        LineKind::ToolError,
                    );
                    self.warn(format!("frus build {target}: {why}"));
                }
            }
            JobKind::Doctor => {
                self.doctor_state = JobState::Done { code };
                if self.checks.is_empty() || code != Some(0) {
                    self.doctor
                        .push(None, "frus doctor gave no report", LineKind::ToolError);
                }
            }
            JobKind::Fix => {
                self.fix_state = JobState::Done { code };
                // What the fix changed, seen at once.
                self.doctor_state = JobState::Idle;
                return self.start_doctor();
            }
        }
        Vec::new()
    }
}

/// `frus doctor --machine`'s answer, read.
fn parse_checks(text: &str) -> Option<Vec<Check>> {
    let value = crate::json::parse(text).ok()?;
    let checks = value
        .as_array()?
        .iter()
        .map(|check| Check {
            name: check.str("name").unwrap_or("").to_string(),
            status: check.str("status").unwrap_or("").to_string(),
            summary: check.str("summary").unwrap_or("").to_string(),
            findings: check
                .get("findings")
                .and_then(crate::json::Value::as_array)
                .unwrap_or_default()
                .iter()
                .map(|f| {
                    (
                        f.get("ok").and_then(crate::json::Value::as_bool),
                        f.str("text").unwrap_or("").to_string(),
                    )
                })
                .collect(),
        })
        .collect();
    Some(checks)
}

/// A line of a program's output as text: without the escape sequences that colour it, and
/// without the carriage returns a progress bar redraws itself with (what follows the last one
/// is what was left on the screen).
pub fn plain(text: &str) -> String {
    let text = text.trim_end_matches(['\r', '\n']);
    let text = text.rsplit('\r').next().unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                // CSI: up to its final byte, a letter or one of @[\]^_`{|}~.
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC (a hyperlink, a title): up to BEL or ESC \.
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            },
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn device(id: &str, name: &str) -> Device {
        Device {
            id: id.into(),
            name: name.into(),
            platform: "windows".into(),
            category: "desktop".into(),
            detail: String::new(),
            emulator: false,
            unavailable: None,
        }
    }

    fn app_with_devices() -> App {
        let mut app = App::new(Some("my_app".into()));
        app.update(Msg::Devices(vec![
            device("windows", "This computer"),
            device("web", "Browser"),
        ]));
        app
    }

    fn key(app: &mut App, input: Input) -> Vec<Effect> {
        app.update(Msg::Input(input))
    }

    #[test]
    fn enter_runs_on_the_selected_device() {
        let mut app = app_with_devices();
        key(&mut app, Input::Down);
        let effects = key(&mut app, Input::Enter);
        assert_eq!(
            effects,
            [Effect::StartRun {
                key: 1,
                device: "web".into()
            }]
        );
        assert_eq!(app.runs.len(), 1);
        assert!(matches!(app.runs[0].status, RunStatus::Building(_)));
        // Not twice on one device.
        assert!(key(&mut app, Input::Enter).is_empty());
        assert!(app.notice.as_ref().unwrap().0.contains("Already running"));
    }

    #[test]
    fn marked_devices_run_together() {
        let mut app = app_with_devices();
        key(&mut app, Input::Down);
        key(&mut app, Input::Char(' '));
        key(&mut app, Input::Up);
        key(&mut app, Input::Char(' '));
        let effects = key(&mut app, Input::Enter);
        let devices: Vec<_> = effects
            .iter()
            .map(|e| match e {
                Effect::StartRun { device, .. } => device.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(devices, ["windows", "web"]);
        assert!(app.marked.is_empty());
        assert!(app.several_devices());
    }

    #[test]
    fn an_unavailable_device_is_not_run_on() {
        let mut app = App::new(None);
        let mut phone = device("A1", "Phone");
        phone.unavailable = Some("not authorised".into());
        app.update(Msg::Devices(vec![phone]));
        assert!(key(&mut app, Input::Enter).is_empty());
        assert_eq!(
            app.notice,
            Some(("Phone cannot be run on: not authorised".into(), true))
        );
    }

    fn started(app: &mut App) {
        key(app, Input::Enter);
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Started { hot: true },
        });
    }

    #[test]
    fn r_reloads_once_the_application_is_up() {
        let mut app = app_with_devices();
        key(&mut app, Input::Enter);
        // Still building: nothing to reload.
        assert!(key(&mut app, Input::Char('r')).is_empty());
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Started { hot: false },
        });
        assert_eq!(
            key(&mut app, Input::Char('r')),
            [Effect::Send {
                key: 1,
                command: RunCommand::Reload
            }]
        );
        assert_eq!(app.runs[0].status, RunStatus::Reloading { full: false });
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Restarted {
                full: false,
                seconds: 0.25,
            },
        });
        assert_eq!(app.runs[0].status, RunStatus::Running { hot: false });
        assert_eq!(app.log.lines.last().unwrap().text, "Reloaded in 0.2 s");
        assert_eq!(
            key(&mut app, Input::Char('R')),
            [Effect::Send {
                key: 1,
                command: RunCommand::Restart
            }]
        );
        // A restart that has not reported yet can be asked again.
        assert_eq!(app.runs[0].status, RunStatus::Reloading { full: true });
        assert_eq!(key(&mut app, Input::Char('R')).len(), 1);
    }

    #[test]
    fn with_hot_reload_a_save_patches_and_r_says_so() {
        let mut app = app_with_devices();
        started(&mut app);
        // Nothing to send: a saved file is patched in by itself, as in `frus run`.
        assert!(key(&mut app, Input::Char('r')).is_empty());
        assert_eq!(app.runs[0].status, RunStatus::Running { hot: true });
        assert!(app
            .notice
            .as_ref()
            .unwrap()
            .0
            .starts_with("Hot reload is on"));
        // A save, patched in.
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Restarted {
                full: false,
                seconds: 0.4,
            },
        });
        assert_eq!(app.log.lines.last().unwrap().text, "Patched in 0.4 s");
        // R still restarts.
        assert_eq!(
            key(&mut app, Input::Char('R')),
            [Effect::Send {
                key: 1,
                command: RunCommand::Restart
            }]
        );
    }

    #[test]
    fn keys_go_to_the_picked_run_when_the_runs_have_the_focus() {
        let mut app = app_with_devices();
        key(&mut app, Input::Down);
        key(&mut app, Input::Char(' '));
        key(&mut app, Input::Up);
        key(&mut app, Input::Char(' '));
        key(&mut app, Input::Enter);
        for run in 1..=2 {
            app.update(Msg::Run {
                key: run,
                event: RunEvent::Started { hot: false },
            });
        }
        // Everywhere from the device list.
        assert_eq!(key(&mut app, Input::Char('s')).len(), 2);
        // To one run from the Runs panel.
        key(&mut app, Input::Tab);
        assert_eq!(app.focus, Focus::Runs);
        key(&mut app, Input::Up);
        assert_eq!(
            key(&mut app, Input::Char('s')),
            [Effect::Send {
                key: 1,
                command: RunCommand::Stop
            }]
        );
    }

    #[test]
    fn a_run_ends_once_and_can_be_started_again() {
        let mut app = app_with_devices();
        started(&mut app);
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Stop { error: None },
        });
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Exited(Some(0)),
        });
        assert_eq!(app.runs[0].status, RunStatus::Stopped { error: None });
        assert_eq!(
            app.log
                .lines
                .iter()
                .filter(|l| l.text.starts_with("The run ended"))
                .count(),
            1
        );
        key(&mut app, Input::Tab);
        assert_eq!(
            key(&mut app, Input::Enter),
            [Effect::StartRun {
                key: 2,
                device: "windows".into()
            }]
        );
    }

    #[test]
    fn a_process_that_dies_without_a_word_is_a_failure() {
        let mut app = app_with_devices();
        key(&mut app, Input::Enter);
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Exited(Some(101)),
        });
        assert_eq!(
            app.runs[0].status,
            RunStatus::Stopped {
                error: Some("frus run ended with exit code 101".into())
            }
        );
    }

    #[test]
    fn the_status_follows_the_build() {
        let mut app = app_with_devices();
        key(&mut app, Input::Enter);
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Output("   Compiling my_app v0.1.0".into()),
        });
        assert_eq!(
            app.runs[0].status,
            RunStatus::Building("Compiling my_app v0.1.0".into())
        );
        assert_eq!(app.log.lines[0].kind, LineKind::Output);
        // Each device in front of its lines, once two run.
        assert_eq!(app.log.lines[0].source, None);
    }

    #[test]
    fn a_scrolled_log_stays_put_as_lines_arrive() {
        let mut app = app_with_devices();
        started(&mut app);
        for i in 0..100 {
            app.update(Msg::Run {
                key: 1,
                event: RunEvent::Log {
                    text: format!("line {i}"),
                    error: false,
                },
            });
        }
        key(&mut app, Input::PageUp);
        let scrolled = app.log.scroll;
        assert!(scrolled > 0);
        app.update(Msg::Run {
            key: 1,
            event: RunEvent::Log {
                text: "more".into(),
                error: false,
            },
        });
        assert_eq!(app.log.scroll, scrolled + 1);
        key(&mut app, Input::End);
        assert_eq!(app.log.scroll, 0);
        key(&mut app, Input::Home);
        assert_eq!(app.log.scroll, 102 - view::pane_height(&app));
    }

    #[test]
    fn devices_come_and_go() {
        let mut app = app_with_devices();
        app.device_cursor = 1;
        app.update(Msg::DeviceRemoved("web".into()));
        assert_eq!(app.devices.len(), 1);
        assert_eq!(app.device_cursor, 0);
        let mut phone = device("A1", "Phone");
        phone.platform = "android".into();
        app.update(Msg::DeviceAdded(phone));
        assert_eq!(app.devices[1].name, "Phone");
        assert_eq!(app.notice, Some(("Phone connected".into(), false)));
    }

    #[test]
    fn the_cursor_stays_on_its_device_as_the_list_changes() {
        // Found on a real terminal: the daemon's first look reports every device again as
        // added, which moved them all to the end of the list, under the cursor.
        let mut app = app_with_devices();
        app.update(Msg::Input(Input::Down));
        app.notice = None;
        for (id, name) in [("windows", "This computer"), ("web", "Browser")] {
            app.update(Msg::DeviceAdded(device(id, name)));
        }
        let ids: Vec<_> = app.devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["windows", "web"]);
        assert_eq!(app.notice, None);
        assert_eq!(app.device_cursor, 1);
        // A new list in another order: the cursor follows its device.
        app.update(Msg::Devices(vec![
            device("A1", "Phone"),
            device("web", "Browser"),
            device("windows", "This computer"),
        ]));
        assert_eq!(app.device_cursor, 1);
        assert_eq!(
            key(&mut app, Input::Enter),
            [Effect::StartRun {
                key: 1,
                device: "web".into()
            }]
        );
    }

    #[test]
    fn enter_on_an_emulator_starts_it() {
        let mut app = app_with_devices();
        app.update(Msg::Emulators(vec![Emulator {
            id: "Pixel_7".into(),
            name: "Pixel 7".into(),
            platform: "android".into(),
        }]));
        key(&mut app, Input::Down);
        key(&mut app, Input::Down);
        assert_eq!(app.rows()[app.device_cursor], Row::Emulator(0));
        assert_eq!(
            key(&mut app, Input::Enter),
            [Effect::LaunchEmulator("Pixel_7".into())]
        );
    }

    #[test]
    fn q_quits_from_anywhere() {
        for tab in ['1', '2', '3'] {
            let mut app = app_with_devices();
            key(&mut app, Input::Char(tab));
            assert_eq!(key(&mut app, Input::Char('q')), [Effect::Quit]);
            assert!(app.quitting);
        }
        let mut app = app_with_devices();
        assert_eq!(key(&mut app, Input::Ctrl('c')), [Effect::Quit]);
    }

    #[test]
    fn a_build_runs_to_its_result() {
        let mut app = app_with_devices();
        key(&mut app, Input::Char('2'));
        key(&mut app, Input::Down);
        assert_eq!(key(&mut app, Input::Enter), [Effect::Build("web".into())]);
        assert!(key(&mut app, Input::Enter).is_empty());
        assert_eq!(
            key(&mut app, Input::Char('c')),
            [Effect::Cancel(JobKind::Build)]
        );
        app.update(Msg::JobLine {
            job: JobKind::Build,
            text: "Compiling".into(),
            error: true,
        });
        app.update(Msg::JobDone {
            job: JobKind::Build,
            code: Some(0),
        });
        assert_eq!(app.build_state, JobState::Done { code: Some(0) });
        assert_eq!(app.build.lines.last().unwrap().text, "frus build web: done");
    }

    #[test]
    fn the_doctor_looks_when_its_tab_opens_and_again_after_a_fix() {
        let mut app = app_with_devices();
        assert_eq!(key(&mut app, Input::Char('3')), [Effect::Doctor]);
        app.update(Msg::JobLine {
            job: JobKind::Doctor,
            text: r#"[{"name":"Rust","status":"ready","summary":"1.96","findings":[{"ok":true,"text":"rustc 1.96"}]},{"name":"Android","status":"missing","summary":"","findings":[{"ok":false,"text":"no SDK"}]}]"#.into(),
            error: false,
        });
        app.update(Msg::JobDone {
            job: JobKind::Doctor,
            code: Some(0),
        });
        let texts: Vec<_> = app.doctor.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "[✓] Rust (1.96)",
                "    • rustc 1.96",
                "[✗] Android",
                "    ✗ no SDK"
            ]
        );
        // Not again on the next visit.
        key(&mut app, Input::Char('1'));
        assert!(key(&mut app, Input::Char('3')).is_empty());
        assert_eq!(key(&mut app, Input::Char('f')), [Effect::Fix]);
        assert_eq!(
            app.update(Msg::JobDone {
                job: JobKind::Fix,
                code: Some(0)
            }),
            [Effect::Doctor]
        );
    }

    #[test]
    fn output_is_made_plain() {
        assert_eq!(
            plain("\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m x\r\n"),
            "   Compiling x"
        );
        assert_eq!(plain("10%\r50%\r100%"), "100%");
        assert_eq!(plain("\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{7}"), "link");
        assert_eq!(plain("a\tb"), "a    b");
    }
}
