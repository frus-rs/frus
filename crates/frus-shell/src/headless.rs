// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Headless entry points** (milestone 828): Rust code that runs with no window — a sync on a
//! schedule, an upload finished after the person left — declared beside the application:
//!
//! ```ignore
//! fn sync(cx: &frus::HeadlessContext) -> frus::anyhow::Result<()> {
//!     let count = cx.prefs().get::<u32>("syncs").unwrap_or(0) + 1;
//!     cx.prefs().set("syncs", count)?;
//!     cx.write_file("last-sync.txt", b"done")?;
//!     Ok(())
//! }
//!
//! frus::main!(App::default(), headless: { "sync" => sync });
//! ```
//!
//! and scheduled from anywhere, the interface or another entry point:
//!
//! ```ignore
//! frus::schedule_job(BackgroundJob::periodic("sync", Duration::from_secs(3600)).requires_network())?;
//! ```
//!
//! An entry point is a plain function. It has no application, no window, no theme and no
//! messages: what it hands the interface goes through storage — the preferences, files in the
//! application's data folder — which the interface reads when it is next built. It may post a
//! notification. It must not wait for the person: nothing on screen can answer.
//!
//! - **Android**: the system's job scheduler runs it, through a job service `frus build` adds
//!   to the package, in the application's process — the running one, or one started for the
//!   job with no activity. Constraints (network, charging, idle) are the scheduler's own; a
//!   periodic job runs at most every 15 minutes, and the system may defer it further.
//! - **The desktop**: a schedule runs on a thread of the application's process while it
//!   runs (an application that must keep running with no window keeps its tray icon). The
//!   program runs one entry point and ends when started with `--frus-job <name>`: what the
//!   system's own scheduler (Task Scheduler, cron, launchd) is given.
//! - **The web and iOS**: [`schedule_job`] answers [`JobError::Unsupported`];
//!   [`run_headless`] runs an entry point in the page or the application.

use std::collections::HashMap;
#[cfg(not(web))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// A headless entry point: what [`main!`](crate::main) is given after `headless:`. `Ok` is a
/// job done; an error is logged, and the job is tried again later (with a growing delay on
/// Android).
pub type HeadlessEntry = fn(&HeadlessContext) -> anyhow::Result<()>;

/// The entry points the application declared, by name.
static ENTRIES: OnceLock<&'static [(&'static str, HeadlessEntry)]> = OnceLock::new();

/// How many entry points run now, in this process.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Records the application's entry points. Called by [`main!`](crate::main).
#[doc(hidden)]
pub fn __register_headless(entries: &'static [(&'static str, HeadlessEntry)]) {
    let _ = ENTRIES.set(entries);
}

fn entry(name: &str) -> Option<HeadlessEntry> {
    ENTRIES
        .get()?
        .iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, entry)| *entry)
}

/// Why a job could not be scheduled, cancelled or run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    /// No headless entry point of that name: `main!` declares them.
    NoSuchEntry(String),
    /// This platform runs no job without a window.
    Unsupported(&'static str),
    /// The system refused it, or the package lacks what it needs.
    Platform(String),
    /// The entry point ran and answered an error.
    Failed(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchEntry(name) => write!(
                f,
                "no headless entry point named {name:?}: declare it in main!(.., headless: {{ .. }})"
            ),
            Self::Unsupported(why) => write!(f, "background jobs are not supported: {why}"),
            Self::Platform(why) => write!(f, "the job could not be scheduled: {why}"),
            Self::Failed(why) => write!(f, "the job failed: {why}"),
        }
    }
}

impl std::error::Error for JobError {}

/// What a headless entry point is handed: the job's name and input, the application's storage,
/// a way to post a notification, and whether the system asked it to stop.
///
/// It may: read and write the [preferences](Self::prefs) and files in the
/// [data folder](Self::data_dir), shared with the interface without a race; post a
/// [notification](Self::notify); use the network (a blocking client, or
/// `pollster::block_on` around a future); [schedule](crate::schedule_job) or
/// [cancel](crate::cancel_job) jobs.
///
/// It may not: show anything, open a window, send the application a message (there may be no
/// application), ask for a permission (only a window can), or run for long — Android stops a
/// job after about ten minutes, and [`is_stopped`](Self::is_stopped) says when it asked.
pub struct HeadlessContext {
    name: String,
    input: Option<String>,
    stop: Arc<AtomicBool>,
    #[cfg(android)]
    android: Option<Arc<android::Handle>>,
}

impl HeadlessContext {
    /// A context for `name`, as a test or [`run_headless`] makes one.
    fn new(name: &str, input: Option<String>, stop: Arc<AtomicBool>) -> Self {
        Self {
            name: name.to_owned(),
            input,
            stop,
            #[cfg(android)]
            android: None,
        }
    }

    /// The entry point's name, as declared and scheduled.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the job was scheduled with ([`BackgroundJob::input`]).
    pub fn input(&self) -> Option<&str> {
        self.input.as_deref()
    }

    /// Whether the system asked the job to stop — its constraints no longer hold, or it ran
    /// too long. An entry point that loops checks it and returns; the job is run again later.
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// The application's preferences: the same store the interface reads, in this process
    /// or another, without a race (see [`prefs`](frus_widgets::prefs)).
    pub fn prefs(&self) -> &'static frus_widgets::Prefs {
        frus_widgets::prefs()
    }

    /// The application's data folder ([`app_data_dir`](crate::app_data_dir)). `None` on the web.
    pub fn data_dir(&self) -> Option<PathBuf> {
        crate::app_data_dir()
    }

    /// Writes `bytes` to `name` in the [data folder](Self::data_dir), whole or not at all: to
    /// a temporary file beside it, then renamed over it, so that the interface reading it at
    /// the same moment reads the old file or the new one, never half of one. Answers the
    /// file's path.
    #[cfg(not(web))]
    pub fn write_file(&self, name: impl AsRef<Path>, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let dir = self
            .data_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no data folder"))?;
        let path = dir.join(name);
        crate::storage::write_atomically(&path, bytes)?;
        Ok(path)
    }

    /// Posts a notification titled `title`, with `body` under it, and returns without waiting:
    /// a tap opens the application. Each entry point has one notification, which a later one
    /// replaces. On Android 13 and later the person must have allowed notifications, which
    /// only the application's window can ask.
    pub fn notify(&self, title: &str, body: &str) -> Result<(), JobError> {
        #[cfg(android)]
        {
            let handle = self
                .android
                .as_ref()
                .ok_or(JobError::Unsupported("not run by the job service"))?;
            android::notify(handle, &self.name, title, body)
        }
        #[cfg(all(desktop, feature = "notifications"))]
        {
            crate::SystemNotification::new(title)
                .body(body)
                .post_desktop()
                .map_err(|err| JobError::Platform(err.to_string()))
        }
        #[cfg(not(any(android, all(desktop, feature = "notifications"))))]
        {
            let _ = (title, body);
            Err(JobError::Unsupported(
                "notifications from a job need the `notifications` feature on the desktop, \
                 and Android",
            ))
        }
    }
}

/// A job: an entry point to run with no window, once or periodically, when its constraints
/// hold. Handed to [`schedule_job`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundJob {
    pub(crate) name: String,
    pub(crate) input: Option<String>,
    pub(crate) period: Option<Duration>,
    pub(crate) delay: Duration,
    pub(crate) network: Network,
    pub(crate) charging: bool,
    pub(crate) idle: bool,
    pub(crate) battery_not_low: bool,
    pub(crate) persisted: bool,
}

/// The network a job waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Network {
    None,
    Any,
    Unmetered,
}

/// The shortest period Android runs a periodic job at; a shorter one is run this often.
pub const MIN_JOB_PERIOD: Duration = Duration::from_secs(15 * 60);

impl BackgroundJob {
    fn new(name: &str, period: Option<Duration>) -> Self {
        Self {
            name: name.to_owned(),
            input: None,
            period,
            delay: Duration::ZERO,
            network: Network::None,
            charging: false,
            idle: false,
            battery_not_low: false,
            persisted: false,
        }
    }

    /// Runs the entry point `name` once, as soon as its constraints hold.
    pub fn once(name: &str) -> Self {
        Self::new(name, None)
    }

    /// Runs the entry point `name` every `every`, while it stays scheduled. Android runs it at
    /// most every [`MIN_JOB_PERIOD`], at a moment of its choosing within each period.
    pub fn periodic(name: &str, every: Duration) -> Self {
        Self::new(name, Some(every))
    }

    /// Text the entry point reads with [`HeadlessContext::input`]: an id, a small JSON.
    pub fn input(mut self, input: impl Into<String>) -> Self {
        self.input = Some(input.into());
        self
    }

    /// Waits at least `delay` before the first run of a job run [once](Self::once).
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Runs only with a network connection.
    pub fn requires_network(mut self) -> Self {
        self.network = Network::Any;
        self
    }

    /// Runs only on a network that is not metered (Wi-Fi rather than mobile data).
    pub fn requires_unmetered_network(mut self) -> Self {
        self.network = Network::Unmetered;
        self
    }

    /// Runs only while the device charges.
    pub fn requires_charging(mut self) -> Self {
        self.charging = true;
        self
    }

    /// Runs only while the device is idle, not in use.
    pub fn requires_idle(mut self) -> Self {
        self.idle = true;
        self
    }

    /// Runs only when the battery is not low.
    pub fn requires_battery_not_low(mut self) -> Self {
        self.battery_not_low = true;
        self
    }

    /// Keeps the job scheduled across a restart of the device (Android). Without it, a
    /// restart forgets it.
    pub fn persisted(mut self) -> Self {
        self.persisted = true;
        self
    }

    /// The entry point it runs.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Schedules `job`, replacing the job of the same entry point if one is scheduled.
///
/// Android hands it to the system's job scheduler, which runs it with the application closed
/// (the package must come from `frus build` or `frus run`, which add the job service). The
/// desktop runs it on a thread while the application runs. The web and iOS answer
/// [`JobError::Unsupported`].
pub fn schedule_job(job: BackgroundJob) -> Result<(), JobError> {
    if ENTRIES.get().is_some() && entry(&job.name).is_none() {
        return Err(JobError::NoSuchEntry(job.name));
    }
    #[cfg(android)]
    return android::schedule(&job);
    #[cfg(desktop)]
    return desktop::schedule(job);
    #[cfg(not(any(android, desktop)))]
    {
        let _ = job;
        Err(JobError::Unsupported(
            "this platform runs no job without a window; run_headless runs one now",
        ))
    }
}

/// Whether a job of the entry point `name` is scheduled: what an application asks at start
/// before scheduling its periodic job, since scheduling again replaces the job and starts its
/// period over.
pub fn is_job_scheduled(name: &str) -> Result<bool, JobError> {
    #[cfg(android)]
    return android::scheduled(name);
    #[cfg(desktop)]
    return Ok(desktop::is_scheduled(name));
    #[cfg(not(any(android, desktop)))]
    {
        let _ = name;
        Err(JobError::Unsupported(
            "this platform runs no job without a window",
        ))
    }
}

/// Cancels the job of the entry point `name`; none scheduled is not an error. A run already
/// started is asked to stop ([`HeadlessContext::is_stopped`]) on the desktop, and finishes on
/// Android.
pub fn cancel_job(name: &str) -> Result<(), JobError> {
    #[cfg(android)]
    return android::cancel(name);
    #[cfg(desktop)]
    {
        desktop::cancel(name);
        Ok(())
    }
    #[cfg(not(any(android, desktop)))]
    {
        let _ = name;
        Err(JobError::Unsupported(
            "this platform runs no job without a window",
        ))
    }
}

/// Runs the entry point `name` now, on this thread, as a job would: for a test, or for work
/// the interface wants done the same way. Call it from a background thread
/// (`Command::perform`), never from the interface's.
pub fn run_headless(name: &str, input: Option<&str>) -> Result<(), JobError> {
    let context = HeadlessContext::new(name, input.map(str::to_owned), Default::default());
    run(context)
}

/// Runs an entry point, catching its panic: what every way of running one ends in.
fn run(context: HeadlessContext) -> Result<(), JobError> {
    run_counted(context, Running::start())
}

/// Counts one run in [`RUNNING`] for as long as it lives.
struct Running(());

impl Running {
    fn start() -> Running {
        RUNNING.fetch_add(1, Ordering::AcqRel);
        Running(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// [`run`], for a run already counted: a scheduled run is counted under the lock its cancel
/// takes, so that once [`cancel_job`] returns, a run has either been counted (and
/// [`wait_for_running`] waits for it) or will never start.
fn run_counted(context: HeadlessContext, running: Running) -> Result<(), JobError> {
    let name = context.name.clone();
    let entry = entry(&name).ok_or_else(|| JobError::NoSuchEntry(name.clone()))?;
    log::info!("background job {name:?}: running");
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| entry(&context)));
    drop(running);
    let result = match ran {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(JobError::Failed(format!("{err:#}"))),
        Err(_) => Err(JobError::Failed("it panicked".to_owned())),
    };
    match &result {
        Ok(()) => log::info!("background job {name:?}: done"),
        Err(err) => log::warn!("background job {name:?}: {err}; it will be tried again"),
    }
    result
}

/// Waits, up to `limit`, for the entry points running in this process to finish: what the
/// Android shell does before it ends the process its activity leaves. A job cut short is run
/// again by the scheduler.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn wait_for_running(limit: Duration) {
    let start = std::time::Instant::now();
    while RUNNING.load(Ordering::Acquire) > 0 && start.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The stop flags of the runs in progress, by the system's job id.
#[cfg_attr(not(android), allow(dead_code))]
fn stops() -> std::sync::MutexGuard<'static, HashMap<i32, Arc<AtomicBool>>> {
    static STOPS: OnceLock<Mutex<HashMap<i32, Arc<AtomicBool>>>> = OnceLock::new();
    STOPS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The program's arguments name an entry point to run (`--frus-job <name>`, with
/// `--frus-job-input <text>`): it is run, and its result is the program's. Called by the
/// `run()` [`main!`](crate::main) generates for an application with headless entry points,
/// before any window.
#[doc(hidden)]
#[cfg(any(desktop, ios))]
pub fn __headless_from_args() -> Option<anyhow::Result<()>> {
    let (name, input) = job_in_args(std::env::args().skip(1))?;
    // iOS has no logger of frus's to set up.
    #[cfg(desktop)]
    crate::logging::init::<HeadlessContext>();
    Some(run_headless(&name, input.as_deref()).map_err(anyhow::Error::new))
}

/// `--frus-job <name>` and `--frus-job-input <text>` among the program's arguments.
#[cfg_attr(not(any(desktop, ios)), allow(dead_code))]
fn job_in_args(args: impl Iterator<Item = String>) -> Option<(String, Option<String>)> {
    let mut name = None;
    let mut input = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--frus-job=") {
            name = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("--frus-job-input=") {
            input = Some(value.to_owned());
        } else if arg == "--frus-job" {
            name = args.next();
        } else if arg == "--frus-job-input" {
            input = args.next();
        }
    }
    name.map(|name| (name, input))
}

/// The desktop: a thread per scheduled job, while the process runs.
#[cfg(desktop)]
mod desktop {
    use super::*;
    use std::sync::Condvar;

    /// A scheduled job's thread is told to end through this.
    #[derive(Default)]
    struct Cancel {
        cancelled: Mutex<bool>,
        wake: Condvar,
        stop: Arc<AtomicBool>,
    }

    impl Cancel {
        /// Waits `delay`, or less when cancelled: whether it was.
        fn wait(&self, delay: Duration) -> bool {
            let guard = self.cancelled.lock().unwrap_or_else(|e| e.into_inner());
            let (guard, _) = self
                .wake
                .wait_timeout_while(guard, delay, |cancelled| !*cancelled)
                .unwrap_or_else(|e| e.into_inner());
            *guard
        }

        /// Counts a run about to start, unless the job was cancelled: both under the lock
        /// [`Cancel::cancel`] takes, so a cancel cannot fall between the wait and the count.
        fn begin(&self) -> Option<Running> {
            let cancelled = self.cancelled.lock().unwrap_or_else(|e| e.into_inner());
            (!*cancelled).then(Running::start)
        }

        fn cancel(&self) {
            *self.cancelled.lock().unwrap_or_else(|e| e.into_inner()) = true;
            self.stop.store(true, Ordering::Release);
            self.wake.notify_all();
        }
    }

    fn scheduled() -> std::sync::MutexGuard<'static, HashMap<String, Arc<Cancel>>> {
        static SCHEDULED: OnceLock<Mutex<HashMap<String, Arc<Cancel>>>> = OnceLock::new();
        SCHEDULED
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The first retry of a failed run waits this long, and each next one twice as long.
    const RETRY: Duration = Duration::from_secs(30);
    /// A failed run is tried this many times again.
    const RETRIES: u32 = 5;

    pub(super) fn schedule(job: BackgroundJob) -> Result<(), JobError> {
        let cancel = Arc::new(Cancel::default());
        if let Some(previous) = scheduled().insert(job.name.clone(), cancel.clone()) {
            previous.cancel();
        }
        std::thread::Builder::new()
            .name(format!("frus-job-{}", job.name))
            .spawn(move || run_schedule(job, cancel))
            .map_err(|err| JobError::Platform(err.to_string()))?;
        Ok(())
    }

    fn run_schedule(job: BackgroundJob, cancel: Arc<Cancel>) {
        let mut wait = job.delay;
        let mut failures = 0;
        loop {
            if cancel.wait(wait) {
                return;
            }
            // The wait may have timed out just as the job was cancelled.
            let Some(running) = cancel.begin() else {
                return;
            };
            let context = HeadlessContext::new(&job.name, job.input.clone(), cancel.stop.clone());
            let failed = run_counted(context, running).is_err();
            wait = match (failed, job.period) {
                (true, _) if failures < RETRIES => {
                    failures += 1;
                    RETRY * 2u32.pow(failures - 1)
                }
                (_, Some(period)) => {
                    failures = 0;
                    period
                }
                (_, None) => break,
            };
        }
        let mut scheduled = scheduled();
        if scheduled
            .get(&job.name)
            .is_some_and(|current| Arc::ptr_eq(current, &cancel))
        {
            scheduled.remove(&job.name);
        }
    }

    pub(super) fn cancel(name: &str) {
        if let Some(cancel) = scheduled().remove(name) {
            cancel.cancel();
        }
    }

    pub(super) fn is_scheduled(name: &str) -> bool {
        scheduled().contains_key(name)
    }

    #[cfg(test)]
    #[test]
    fn a_run_is_not_counted_once_its_job_is_cancelled() {
        // The schedule's wait timed out, then the cancel came before the run was counted.
        let cancel = Cancel::default();
        let running = cancel.begin();
        assert!(running.is_some(), "a live job's run is counted");
        drop(running);
        cancel.cancel();
        assert!(
            cancel.begin().is_none(),
            "a cancelled job's run never starts"
        );
    }
}

/// Android: the system's job scheduler, through `dev.frus.FrusJobService`.
#[cfg(android)]
mod android {
    use super::*;
    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    /// The service's class and the application's context, while a job has run in this
    /// process.
    pub(super) struct Handle {
        vm: JavaVM,
        context: GlobalRef,
        class: GlobalRef,
    }

    static HANDLE: Mutex<Option<Arc<Handle>>> = Mutex::new(None);

    const SERVICE: &str = "dev.frus.FrusJobService";

    fn platform(err: impl std::fmt::Display) -> JobError {
        JobError::Platform(err.to_string())
    }

    /// Clears the Java exception a failed call left, and says what it was.
    fn cleared(env: &mut JNIEnv, err: jni::errors::Error) -> JobError {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        platform(err)
    }

    /// The service's class and a context: the ones a job was run with, or the activity's,
    /// whose class loader holds the package's code.
    fn handle() -> Result<Arc<Handle>, JobError> {
        if let Some(handle) = HANDLE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(handle);
        }
        let context = ndk_context::android_context();
        if context.vm().is_null() || context.context().is_null() {
            return Err(JobError::Platform("the application has not started".into()));
        }
        let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.map_err(platform)?;
        let mut env = vm.attach_current_thread().map_err(platform)?;
        let activity = unsafe { JObject::from_raw(context.context().cast()) };
        let loaded = (|| {
            let loader = env
                .call_method(
                    &activity,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[],
                )?
                .l()?;
            let name = env.new_string(SERVICE)?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let app = env
                .call_method(
                    &activity,
                    "getApplicationContext",
                    "()Landroid/content/Context;",
                    &[],
                )?
                .l()?;
            Ok::<_, jni::errors::Error>((env.new_global_ref(&app)?, env.new_global_ref(&class)?))
        })();
        let (context, class) = match loaded {
            Ok(loaded) => loaded,
            Err(err) => {
                let _ = cleared(&mut env, err);
                return Err(JobError::Platform(
                    "the package has no job service: build it with `frus build` or `frus run`, \
                     which add it to an application with headless entry points"
                        .into(),
                ));
            }
        };
        drop(env);
        let handle = Arc::new(Handle { vm, context, class });
        *HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle.clone());
        Ok(handle)
    }

    fn millis(duration: Duration) -> i64 {
        i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
    }

    pub(super) fn schedule(job: &BackgroundJob) -> Result<(), JobError> {
        let handle = handle()?;
        let mut env = handle.vm.attach_current_thread().map_err(platform)?;
        let class: &JClass = handle.class.as_obj().into();
        let name = env.new_string(&job.name).map_err(platform)?;
        let input = match &job.input {
            Some(input) => JObject::from(env.new_string(input).map_err(platform)?),
            None => JObject::null(),
        };
        let network = match job.network {
            Network::None => 0,
            Network::Any => 1,
            Network::Unmetered => 2,
        };
        let taken = env
            .call_static_method(
                class,
                "schedule",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;JJIZZZZ)I",
                &[
                    JValue::Object(handle.context.as_obj()),
                    JValue::Object(&name),
                    JValue::Object(&input),
                    JValue::Long(job.period.map(millis).unwrap_or(0)),
                    JValue::Long(millis(job.delay)),
                    JValue::Int(network),
                    JValue::Bool(job.charging.into()),
                    JValue::Bool(job.idle.into()),
                    JValue::Bool(job.battery_not_low.into()),
                    JValue::Bool(job.persisted.into()),
                ],
            )
            .and_then(|value| value.i());
        match taken {
            Ok(1) => {
                log::info!("background job {:?}: scheduled", job.name);
                Ok(())
            }
            Ok(_) => Err(JobError::Platform("the job scheduler refused it".into())),
            Err(err) => Err(cleared(&mut env, err)),
        }
    }

    pub(super) fn scheduled(name: &str) -> Result<bool, JobError> {
        let handle = handle()?;
        let mut env = handle.vm.attach_current_thread().map_err(platform)?;
        let class: &JClass = handle.class.as_obj().into();
        let name = env.new_string(name).map_err(platform)?;
        env.call_static_method(
            class,
            "scheduled",
            "(Landroid/content/Context;Ljava/lang/String;)Z",
            &[
                JValue::Object(handle.context.as_obj()),
                JValue::Object(&name),
            ],
        )
        .and_then(|value| value.z())
        .map_err(|err| cleared(&mut env, err))
    }

    pub(super) fn cancel(name: &str) -> Result<(), JobError> {
        let handle = handle()?;
        let mut env = handle.vm.attach_current_thread().map_err(platform)?;
        let class: &JClass = handle.class.as_obj().into();
        let name = env.new_string(name).map_err(platform)?;
        env.call_static_method(
            class,
            "cancel",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[
                JValue::Object(handle.context.as_obj()),
                JValue::Object(&name),
            ],
        )
        .map(drop)
        .map_err(|err| cleared(&mut env, err))
    }

    pub(super) fn notify(
        handle: &Handle,
        entry: &str,
        title: &str,
        body: &str,
    ) -> Result<(), JobError> {
        let mut env = handle.vm.attach_current_thread().map_err(platform)?;
        let class: &JClass = handle.class.as_obj().into();
        let id = entry.bytes().fold(17i32, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(byte as i32)
        });
        let title = env.new_string(title).map_err(platform)?;
        let body = env.new_string(body).map_err(platform)?;
        let posted = env
            .call_static_method(
                class,
                "notify",
                "(Landroid/content/Context;ILjava/lang/String;Ljava/lang/String;)I",
                &[
                    JValue::Object(handle.context.as_obj()),
                    JValue::Int(id),
                    JValue::Object(&title),
                    JValue::Object(&body),
                ],
            )
            .and_then(|value| value.i());
        match posted {
            Ok(1) => Ok(()),
            Ok(-1) => Err(JobError::Platform(
                "the person has not allowed notifications (ask from the window)".into(),
            )),
            Ok(_) => Err(JobError::Platform(
                "the system has no notification service".into(),
            )),
            Err(err) => Err(cleared(&mut env, err)),
        }
    }

    /// What `FrusJobService.nativeRun` answers.
    const DONE: i32 = 0;
    const RETRY: i32 = 1;
    const NO_SUCH_ENTRY: i32 = 2;

    /// Runs a job for the service, on the service's thread.
    ///
    /// # Safety
    ///
    /// The arguments are the ones the Java VM hands a static native method.
    pub(super) unsafe fn run_for_service(
        env: *mut jni::sys::JNIEnv,
        class: jni::sys::jclass,
        context: jni::sys::jobject,
        name: jni::sys::jstring,
        input: jni::sys::jstring,
        files: jni::sys::jstring,
        job: i32,
    ) -> i32 {
        let Ok(mut env) = (unsafe { JNIEnv::from_raw(env) }) else {
            return RETRY;
        };
        let class = unsafe { JClass::from_raw(class) };
        let context = unsafe { JObject::from_raw(context) };
        let text = |env: &mut JNIEnv, raw: jni::sys::jstring| -> Option<String> {
            if raw.is_null() {
                return None;
            }
            let string = unsafe { JString::from_raw(raw) };
            env.get_string(&string).ok().map(Into::into)
        };
        let Some(name) = text(&mut env, name) else {
            return NO_SUCH_ENTRY;
        };
        let input = text(&mut env, input);
        // What the activity would have recorded: the application's folder, and its storage.
        if let Some(files) = text(&mut env, files) {
            crate::storage::android::record_files(PathBuf::from(&files));
            crate::data_dir::set_android_files(PathBuf::from(files));
        }
        if let Ok(vm) = env.get_java_vm() {
            crate::storage::android::record_vm(&vm);
        }
        crate::storage::__install_storage();
        crate::logging::init::<HeadlessContext>();
        let handle = (|| {
            Some(Arc::new(Handle {
                vm: env.get_java_vm().ok()?,
                context: env.new_global_ref(&context).ok()?,
                class: env.new_global_ref(&class).ok()?,
            }))
        })();
        if let Some(handle) = &handle {
            *HANDLE.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle.clone());
        }
        if entry(&name).is_none() {
            log::error!("background job {name:?}: no headless entry point of that name");
            return NO_SUCH_ENTRY;
        }
        let stop = Arc::new(AtomicBool::new(false));
        stops().insert(job, stop.clone());
        let mut context = HeadlessContext::new(&name, input, stop);
        context.android = handle;
        let result = run(context);
        stops().remove(&job);
        match result {
            Ok(()) => DONE,
            Err(_) => RETRY,
        }
    }
}

/// Runs a job for Android's job service. Called by the native method [`main!`](crate::main)
/// generates for an application with headless entry points.
///
/// # Safety
///
/// The arguments are the ones the Java VM hands `FrusJobService.nativeRun`.
#[doc(hidden)]
#[cfg(android)]
#[allow(clippy::too_many_arguments)]
pub unsafe fn __headless_android_run(
    env: *mut std::ffi::c_void,
    class: *mut std::ffi::c_void,
    context: *mut std::ffi::c_void,
    name: *mut std::ffi::c_void,
    input: *mut std::ffi::c_void,
    files: *mut std::ffi::c_void,
    job: i32,
    entries: &'static [(&'static str, HeadlessEntry)],
) -> i32 {
    __register_headless(entries);
    unsafe {
        android::run_for_service(
            env.cast(),
            class.cast(),
            context.cast(),
            name.cast(),
            input.cast(),
            files.cast(),
            job,
        )
    }
}

/// Asks the run of job `job` to stop: Android's job service was told to.
#[doc(hidden)]
#[cfg(android)]
pub fn __headless_android_stop(job: i32) {
    if let Some(stop) = stops().get(&job) {
        stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counting(cx: &HeadlessContext) -> anyhow::Result<()> {
        COUNT.fetch_add(1, Ordering::SeqCst);
        if cx.input() == Some("fail") {
            anyhow::bail!("asked to fail");
        }
        Ok(())
    }

    fn panicking(_: &HeadlessContext) -> anyhow::Result<()> {
        panic!("a job's panic stays in the job")
    }

    fn ticking(_: &HeadlessContext) -> anyhow::Result<()> {
        TICKS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static TICKS: AtomicUsize = AtomicUsize::new(0);

    static ENTRIES_FOR_TEST: &[(&str, HeadlessEntry)] = &[
        ("count", counting as HeadlessEntry),
        ("panic", panicking as HeadlessEntry),
        ("tick", ticking as HeadlessEntry),
    ];

    #[test]
    fn an_entry_point_runs_by_name_and_reports_its_result() {
        __register_headless(ENTRIES_FOR_TEST);
        let before = COUNT.load(Ordering::SeqCst);
        assert_eq!(run_headless("count", None), Ok(()));
        assert!(matches!(
            run_headless("count", Some("fail")),
            Err(JobError::Failed(why)) if why.contains("asked to fail")
        ));
        assert_eq!(COUNT.load(Ordering::SeqCst), before + 2);
        assert_eq!(
            run_headless("missing", None),
            Err(JobError::NoSuchEntry("missing".into()))
        );
        assert!(matches!(
            run_headless("panic", None),
            Err(JobError::Failed(_))
        ));
        assert_eq!(
            run_headless("count", None),
            Ok(()),
            "a panic leaves the next run be"
        );
    }

    #[test]
    fn the_job_s_arguments_are_read() {
        let args = |list: &[&str]| job_in_args(list.iter().map(|s| s.to_string()));
        assert_eq!(args(&[]), None);
        assert_eq!(args(&["--other"]), None);
        assert_eq!(args(&["--frus-job", "sync"]), Some(("sync".into(), None)));
        assert_eq!(
            args(&["--frus-job=sync", "--frus-job-input", "{\"id\":3}"]),
            Some(("sync".into(), Some("{\"id\":3}".into())))
        );
        assert_eq!(
            args(&["--frus-job-input=x", "--frus-job", "up"]),
            Some(("up".into(), Some("x".into())))
        );
    }

    #[test]
    fn a_job_is_built_with_its_constraints() {
        let job = BackgroundJob::periodic("sync", Duration::from_secs(3600))
            .input("all")
            .requires_unmetered_network()
            .requires_charging()
            .requires_battery_not_low()
            .persisted();
        assert_eq!(job.name(), "sync");
        assert_eq!(job.period, Some(Duration::from_secs(3600)));
        assert_eq!(job.network, Network::Unmetered);
        assert!(job.charging && job.battery_not_low && job.persisted && !job.idle);
        let once = BackgroundJob::once("upload")
            .delay(Duration::from_secs(5))
            .requires_network()
            .requires_idle();
        assert_eq!(once.period, None);
        assert_eq!(once.delay, Duration::from_secs(5));
        assert_eq!(once.network, Network::Any);
        assert!(once.idle);
    }

    #[cfg(desktop)]
    #[test]
    fn the_desktop_runs_a_schedule_until_it_is_cancelled() {
        __register_headless(ENTRIES_FOR_TEST);
        assert_eq!(
            schedule_job(BackgroundJob::once("nope")),
            Err(JobError::NoSuchEntry("nope".into()))
        );
        schedule_job(BackgroundJob::periodic("tick", Duration::from_millis(20))).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while TICKS.load(Ordering::SeqCst) < 3 {
            assert!(std::time::Instant::now() < deadline, "the job never ran");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(is_job_scheduled("tick"), Ok(true));
        cancel_job("tick").unwrap();
        assert_eq!(is_job_scheduled("tick"), Ok(false));
        wait_for_running(Duration::from_secs(5));
        let after = TICKS.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            TICKS.load(Ordering::SeqCst),
            after,
            "nothing runs once cancelled"
        );
    }

    #[test]
    fn a_stop_reaches_the_running_job() {
        let stop = Arc::new(AtomicBool::new(false));
        stops().insert(7, stop.clone());
        let context = HeadlessContext::new("x", None, stop);
        assert!(!context.is_stopped());
        stops().get(&7).unwrap().store(true, Ordering::Release);
        assert!(context.is_stopped());
        stops().remove(&7);
    }
}
