// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-background-example` — a job that runs with the application closed (milestone 828).
//!
//! `sync` is a **headless entry point**: a plain function, declared beside the application in
//! `frus::main!`, that the system runs with no window. It counts its runs in the preferences,
//! writes what it did to a file in the application's data folder, and posts a notification.
//! The window schedules it, and shows the count and the file whenever it is built: after the
//! application is opened again, or at once when the job runs while the window is open.
//!
//! - Android: the first start (or *Schedule*) hands the job to the system's job scheduler,
//!   every 15 minutes (the shortest period it allows), with the application closed or not.
//! - The desktop: the same schedule runs it on a thread while the window is open, and
//!   `frus-background-example --frus-job sync` runs it once with no window, as the
//!   system's own scheduler would.

#![deny(missing_docs)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frus::{
    button, column, text, Align, BackgroundJob, Container, FrusApp, HeadlessContext, Justify,
    MediaQuery, Size, State, StateContext, StatefulWidget, Variant, Widget,
};

/// The job's name, as `main!` declares it and the window schedules it.
const SYNC: &str = "sync";
/// The preference the job counts its runs in.
const RUNS: &str = "sync.runs";
/// What the job did last, in the application's data folder.
#[cfg(not(target_arch = "wasm32"))]
const RESULT_FILE: &str = "sync-result.txt";
/// What the job did last, on the web (which has no files): a preference.
#[cfg(target_arch = "wasm32")]
const RESULT_PREF: &str = "sync.result";

/// **The headless entry point**: no window, no application; what it leaves for the window
/// goes through storage.
fn sync(cx: &HeadlessContext) -> frus::anyhow::Result<()> {
    let runs = cx.prefs().get::<u64>(RUNS).unwrap_or(0) + 1;
    let line = format!("run {runs}, at {} UTC", clock(SystemTime::now()));
    // The file first, then the count the window reads: whoever sees the new count finds the
    // file that goes with it.
    #[cfg(not(target_arch = "wasm32"))]
    cx.write_file(RESULT_FILE, line.as_bytes())?;
    #[cfg(target_arch = "wasm32")]
    cx.prefs().set(RESULT_PREF, &line)?;
    cx.prefs().set(RUNS, runs)?;
    frus::log::info!("{SYNC}: {line}");
    if let Err(why) = cx.notify("Background job", &line) {
        frus::log::info!("{SYNC}: no notification ({why})");
    }
    Ok(())
}

/// `HH:MM:SS` of `at`, in UTC.
fn clock(at: SystemTime) -> String {
    let seconds = at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// What the job wrote last, or `None` before its first run.
fn last_result() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::read_to_string(frus::app_data_dir()?.join(RESULT_FILE)).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        frus::prefs().get(RESULT_PREF)
    }
}

/// The window: the job's result, and buttons to schedule it.
#[derive(Clone)]
struct Jobs;

/// What the window keeps: the answer to the last button pressed.
#[derive(Default)]
struct JobsState {
    status: String,
}

impl JobsState {
    fn schedule(&mut self) {
        let job = BackgroundJob::periodic(SYNC, frus::MIN_JOB_PERIOD).persisted();
        self.status = match frus::schedule_job(job) {
            Ok(()) => "Scheduled every 15 minutes, with the application closed or not.".into(),
            Err(why) => why.to_string(),
        };
    }

    fn once(&mut self) {
        let job = BackgroundJob::once(SYNC).delay(Duration::from_secs(1));
        self.status = match frus::schedule_job(job) {
            Ok(()) => "Runs once, in a moment.".into(),
            Err(why) => why.to_string(),
        };
    }

    fn cancel(&mut self) {
        self.status = match frus::cancel_job(SYNC) {
            Ok(()) => "Cancelled.".into(),
            Err(why) => why.to_string(),
        };
    }
}

impl StatefulWidget for Jobs {
    type State = JobsState;

    fn create_state(&self) -> JobsState {
        JobsState::default()
    }
}

impl State for JobsState {
    type Widget = Jobs;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        // Built again whenever the count changes, from the job in this process or not.
        let runs = cx.use_pref::<u64>(RUNS, 0).get();
        let result = last_result().unwrap_or_else(|| "The job has not run yet.".into());
        let Size { width, height } = MediaQuery::of().size;
        let content = column![
            text("Background job").size(28.0),
            text(format!("Runs so far: {runs}")).size(20.0),
            text(result),
            button(
                "Schedule every 15 minutes",
                cx.callback(JobsState::schedule)
            )
            .variant(Variant::Filled),
            button("Run once", cx.callback(JobsState::once)).variant(Variant::Outlined),
            button("Cancel", cx.callback(JobsState::cancel)).variant(Variant::Outlined),
            text(self.status.clone()),
        ]
        .gap(16.0)
        .align(Align::Center);
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(cx.theme().background)
                .child(
                    column![content]
                        .width(width)
                        .height(height)
                        .justify(Justify::Center)
                        .align(Align::Center),
                ),
        )
    }
}

// The application, and its headless entry point by name.
/// The application starts: it says what the job did while it was closed, and schedules the
/// job unless it already is (scheduling again would start its period over).
fn app() -> FrusApp {
    let runs = frus::prefs().get::<u64>(RUNS).unwrap_or(0);
    let last = last_result().unwrap_or_else(|| "nothing yet".into());
    frus::log::info!("opened: the job ran {runs} times; last: {last}");
    if frus::is_job_scheduled(SYNC) == Ok(false) {
        let mut state = JobsState::default();
        state.schedule();
        frus::log::info!("{}", state.status);
    }
    FrusApp::stateful(Jobs).title("frus — background job")
}

frus::main!(app(), headless: { "sync" => sync });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_utc() {
        let at = UNIX_EPOCH + Duration::from_secs(86_400 * 3 + 3600 * 13 + 60 * 5 + 9);
        assert_eq!(clock(at), "13:05:09");
    }
}
