// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Reading QR codes and barcodes from the camera** (milestone 798, feature `scanner`).
//!
//! ```no_run
//! use frus_shell::Scanner;
//! use frus_widgets::StatefulWidget;
//!
//! let scanner = Scanner::new().on_detect(|capture| {
//!     for code in &capture.barcodes {
//!         println!("{:?}: {:?}", code.format, code.raw_value);
//!     }
//! });
//! let _widget = scanner.into_widget();
//! ```
//!
//! How it stays light:
//!
//! - the camera opens at [`ResolutionPreset::Medium`] with no picture stream;
//! - a frame is read only when the reader is idle **and** the
//!   [detection timeout](ScannerController::detection_timeout) has passed since the last one
//!   (250 ms by default): its brightness, shrunk to at most 720 pixels a side, is copied and
//!   handed to a thread of the scanner's own, which decodes it; the camera's thread never
//!   waits for a decode;
//! - nothing runs before [`ScannerController::start`] or after
//!   [`stop`](ScannerController::stop): no thread, no camera.
//!
//! The codes are read by [rxing](https://crates.io/crates/rxing), in pure Rust. The `scanner`
//! feature reads QR codes; `scanner-all-formats` adds every format rxing reads (EAN, UPC,
//! Code 128, Data Matrix, PDF417, Aztec...), about a megabyte more.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use frus_widgets::{
    BoxFit, Color, FrameOrientation, State, StateContext, StatefulWidget, TaskHandle, Widget,
};

use crate::camera::{
    available_cameras, CameraController, CameraError, CameraImage, CameraLensDirection, FlashMode,
    ResolutionPreset,
};
use crate::CameraPreview;

/// The kinds of code a scanner reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BarcodeFormat {
    /// QR code.
    QrCode,
    /// Aztec.
    Aztec,
    /// Codabar.
    Codabar,
    /// Code 39.
    Code39,
    /// Code 93.
    Code93,
    /// Code 128.
    Code128,
    /// Data Matrix.
    DataMatrix,
    /// EAN-8.
    Ean8,
    /// EAN-13.
    Ean13,
    /// ITF (Interleaved 2 of 5).
    Itf,
    /// PDF417.
    Pdf417,
    /// UPC-A.
    UpcA,
    /// UPC-E.
    UpcE,
    /// A format with no name here.
    Unknown,
}

impl BarcodeFormat {
    fn of(format: &rxing::BarcodeFormat) -> Self {
        use rxing::BarcodeFormat as F;
        match format {
            F::QR_CODE | F::MICRO_QR_CODE | F::RECTANGULAR_MICRO_QR_CODE => Self::QrCode,
            F::AZTEC => Self::Aztec,
            F::CODABAR => Self::Codabar,
            F::CODE_39 => Self::Code39,
            F::CODE_93 => Self::Code93,
            F::CODE_128 => Self::Code128,
            F::DATA_MATRIX => Self::DataMatrix,
            F::EAN_8 => Self::Ean8,
            F::EAN_13 => Self::Ean13,
            F::ITF => Self::Itf,
            F::PDF_417 => Self::Pdf417,
            F::UPC_A => Self::UpcA,
            F::UPC_E => Self::UpcE,
            _ => Self::Unknown,
        }
    }
}

/// A code the scanner read.
#[derive(Clone, Debug, PartialEq)]
pub struct Barcode {
    /// What kind of code.
    pub format: BarcodeFormat,
    /// Its content as text, when it is text.
    pub raw_value: Option<String>,
    /// Its content as bytes, when the format keeps them.
    pub raw_bytes: Vec<u8>,
    /// Where it was found, in pixels of the frame as the preview shows it (upright, of
    /// [`BarcodeCapture::size`]): a QR code's finder patterns, a barcode's ends.
    pub corners: Vec<(f32, f32)>,
}

/// What one frame gave: the codes, and the size of the picture their corners refer to.
#[derive(Clone, Debug, PartialEq)]
pub struct BarcodeCapture {
    /// The codes read, at least one.
    pub barcodes: Vec<Barcode>,
    /// The frame's size, upright, as the preview shows it.
    pub size: (u32, u32),
}

/// How often the codes in view are reported.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DetectionSpeed {
    /// A code once, until another is seen. The default.
    #[default]
    NoDuplicates,
    /// Every read, at most once per [detection timeout](ScannerController::detection_timeout).
    Normal,
    /// Every frame the reader is free for, the timeout ignored. The heaviest.
    Unrestricted,
}

/// Reads the codes in a picture's brightness: `luma`, `width × height` bytes, a row after
/// another. `formats` empty reads every format the build has (QR codes only without
/// `scanner-all-formats`).
///
/// ```
/// let blank = vec![255u8; 64 * 64];
/// assert!(frus_shell::decode_luma(&blank, 64, 64, &[]).is_empty());
/// ```
pub fn decode_luma(
    luma: &[u8],
    width: u32,
    height: u32,
    formats: &[BarcodeFormat],
) -> Vec<Barcode> {
    use rxing::{BinaryBitmap, DecodeHints, Reader};
    if width == 0 || height == 0 || luma.len() < width as usize * height as usize {
        return Vec::new();
    }
    // Read in place: rxing borrows the brightness.
    let Ok(source) = rxing::Luma8Source::new_with_slice(
        &luma[..width as usize * height as usize],
        width,
        height,
    ) else {
        return Vec::new();
    };
    let mut bitmap = BinaryBitmap::new(rxing::common::HybridBinarizer::new(source));
    // The readers the build has: QR codes, and every other format with `scanner-all-formats`.
    let mut reader = rxing::MultiFormatReader::default();
    let Ok(result) = reader.decode_with_hints(&mut bitmap, &DecodeHints::default()) else {
        return Vec::new();
    };
    let format = BarcodeFormat::of(result.getBarcodeFormat());
    if !formats.is_empty() && !formats.contains(&format) {
        return Vec::new();
    }
    let text = result.getText();
    vec![Barcode {
        format,
        raw_value: (!text.is_empty()).then(|| text.to_string()),
        raw_bytes: result.getRawBytes().to_vec(),
        corners: result.getPoints().iter().map(|p| (p.x, p.y)).collect(),
    }]
}

/// A frame waiting for the reader.
struct Job {
    luma: Vec<u8>,
    width: u32,
    height: u32,
    factor: u32,
    frame: (u32, u32),
    orientation: FrameOrientation,
}

/// Between the camera's thread, which hands frames, and the reader's, which decodes them.
struct Exchange {
    job: Mutex<Option<Job>>,
    ready: Condvar,
    /// The reader is decoding: frames are not even looked at.
    busy: AtomicBool,
    stop: AtomicBool,
    last: Mutex<Option<web_time::Instant>>,
    /// The codes last reported, for [`DetectionSpeed::NoDuplicates`].
    reported: Mutex<Vec<String>>,
}

struct ScannerInner {
    facing: Mutex<CameraLensDirection>,
    formats: Mutex<Vec<BarcodeFormat>>,
    speed: Mutex<DetectionSpeed>,
    timeout: Mutex<Duration>,
    torch: AtomicBool,
    camera: Mutex<Option<CameraController>>,
    exchange: Mutex<Option<Arc<Exchange>>>,
    sender: async_channel::Sender<BarcodeCapture>,
    receiver: async_channel::Receiver<BarcodeCapture>,
}

/// **A code scanner**: the camera, a reader of the codes it sees, and their reports.
///
/// Cheap to clone; the camera closes once [`stop`](Self::stop) is called or the last clone is
/// dropped. The [`Scanner`] widget makes one of its own when it is given none.
///
/// ```no_run
/// use std::time::Duration;
/// use frus_shell::{DetectionSpeed, ScannerController};
///
/// # async fn f() -> Result<(), frus_shell::CameraError> {
/// let scanner = ScannerController::new()
///     .detection_speed(DetectionSpeed::Normal)
///     .detection_timeout(Duration::from_millis(500));
/// scanner.start().await?;
/// while let Some(capture) = scanner.next().await {
///     println!("{:?}", capture.barcodes[0].raw_value);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ScannerController {
    inner: Arc<ScannerInner>,
}

impl std::fmt::Debug for ScannerController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScannerController")
            .field("running", &self.is_running())
            .finish()
    }
}

impl Default for ScannerController {
    fn default() -> Self {
        Self::new()
    }
}

/// The longest side of the brightness handed to the reader.
const READ_SIDE: u32 = 720;

impl ScannerController {
    /// A scanner of the back camera, reporting each code once.
    pub fn new() -> Self {
        let (sender, receiver) = async_channel::bounded(4);
        Self {
            inner: Arc::new(ScannerInner {
                facing: Mutex::new(CameraLensDirection::Back),
                formats: Mutex::new(Vec::new()),
                speed: Mutex::new(DetectionSpeed::default()),
                timeout: Mutex::new(Duration::from_millis(250)),
                torch: AtomicBool::new(false),
                camera: Mutex::new(None),
                exchange: Mutex::new(None),
                sender,
                receiver,
            }),
        }
    }

    /// Which camera: the back one by default.
    pub fn facing(self, facing: CameraLensDirection) -> Self {
        *lock(&self.inner.facing) = facing;
        self
    }

    /// The formats reported; all by default.
    pub fn formats(self, formats: impl IntoIterator<Item = BarcodeFormat>) -> Self {
        *lock(&self.inner.formats) = formats.into_iter().collect();
        self
    }

    /// How often codes are reported.
    pub fn detection_speed(self, speed: DetectionSpeed) -> Self {
        *lock(&self.inner.speed) = speed;
        self
    }

    /// The least time between two frames read: 250 ms by default. Shorter reads sooner and
    /// costs more.
    pub fn detection_timeout(self, timeout: Duration) -> Self {
        *lock(&self.inner.timeout) = timeout;
        self
    }

    /// The torch on from the start.
    pub fn torch_enabled(self, on: bool) -> Self {
        self.inner.torch.store(on, Ordering::Relaxed);
        self
    }

    /// Whether the camera is open and the reader running.
    pub fn is_running(&self) -> bool {
        lock(&self.inner.exchange).is_some()
    }

    /// The camera, while it is open.
    pub fn camera(&self) -> Option<CameraController> {
        lock(&self.inner.camera).clone()
    }

    /// Asks for the camera permission when needed, opens the camera and starts reading.
    pub async fn start(&self) -> Result<(), CameraError> {
        if self.is_running() {
            return Ok(());
        }
        let facing = *lock(&self.inner.facing);
        let cameras = available_cameras().await?;
        let description = cameras
            .iter()
            .find(|c| c.lens_direction == facing)
            .or_else(|| cameras.first())
            .cloned()
            .ok_or(CameraError::NotFound)?;
        let camera = CameraController::new(description, ResolutionPreset::Medium).without_stills();
        camera.initialize().await?;
        if self.inner.torch.load(Ordering::Relaxed) {
            if let Err(err) = camera.set_flash_mode(FlashMode::Torch) {
                log::warn!("scanner: no torch ({err})");
            }
        }
        let exchange = Arc::new(Exchange {
            job: Mutex::new(None),
            ready: Condvar::new(),
            busy: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            last: Mutex::new(None),
            reported: Mutex::new(Vec::new()),
        });
        self.start_reader(&camera, exchange.clone())?;
        *lock(&self.inner.camera) = Some(camera);
        *lock(&self.inner.exchange) = Some(exchange);
        Ok(())
    }

    fn start_reader(
        &self,
        camera: &CameraController,
        exchange: Arc<Exchange>,
    ) -> Result<(), CameraError> {
        let speed = *lock(&self.inner.speed);
        let timeout = match speed {
            DetectionSpeed::Unrestricted => Duration::ZERO,
            _ => *lock(&self.inner.timeout),
        };
        let formats = lock(&self.inner.formats).clone();
        let sender = self.inner.sender.clone();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let reader = exchange.clone();
            std::thread::Builder::new()
                .name("frus-scanner".into())
                .spawn(move || read_frames(&reader, &formats, speed, &sender))
                .map_err(|e| CameraError::Platform(e.to_string()))?;
            camera.start_image_stream(move |image| {
                hand_over(&exchange, image, timeout);
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            // No thread on the web: the frame is read where it arrives, still no more often
            // than the timeout says.
            camera.start_image_stream(move |image| {
                if hand_over(&exchange, image, timeout) {
                    if let Some(job) = lock(&exchange.job).take() {
                        decode_job(&exchange, job, &formats, speed, &sender);
                    }
                    exchange.busy.store(false, Ordering::Release);
                }
            })
        }
    }

    /// The next codes read; `None` once the scanner is stopped. Await it on the interface's
    /// thread: the [`Scanner`] widget does.
    pub async fn next(&self) -> Option<BarcodeCapture> {
        self.inner.receiver.recv().await.ok()
    }

    /// Lights the torch, or puts it out.
    pub fn set_torch(&self, on: bool) -> Result<(), CameraError> {
        self.inner.torch.store(on, Ordering::Relaxed);
        match self.camera() {
            Some(camera) => {
                camera.set_flash_mode(if on { FlashMode::Torch } else { FlashMode::Off })
            }
            None => Ok(()),
        }
    }

    /// Whether the torch is on.
    pub fn torch(&self) -> bool {
        self.inner.torch.load(Ordering::Relaxed)
    }

    /// Switches between the back and the front camera.
    pub async fn switch_camera(&self) -> Result<(), CameraError> {
        let facing = {
            let mut facing = lock(&self.inner.facing);
            *facing = match *facing {
                CameraLensDirection::Front => CameraLensDirection::Back,
                _ => CameraLensDirection::Front,
            };
            *facing
        };
        let Some(camera) = self.camera() else {
            return Ok(());
        };
        let cameras = available_cameras().await?;
        if let Some(description) = cameras.into_iter().find(|c| c.lens_direction == facing) {
            camera.set_description(description).await?;
        }
        Ok(())
    }

    /// Stops reading and closes the camera.
    pub fn stop(&self) {
        if let Some(exchange) = lock(&self.inner.exchange).take() {
            exchange.stop.store(true, Ordering::Release);
            exchange.ready.notify_all();
        }
        if let Some(camera) = lock(&self.inner.camera).take() {
            camera.dispose();
        }
    }
}

impl Drop for ScannerInner {
    fn drop(&mut self) {
        if let Some(exchange) = lock(&self.exchange).take() {
            exchange.stop.store(true, Ordering::Release);
            exchange.ready.notify_all();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// On the camera's thread: copies the frame's brightness for the reader when it is free and
/// the timeout has passed. Answers whether it did.
fn hand_over(exchange: &Exchange, image: &CameraImage<'_>, timeout: Duration) -> bool {
    if exchange.busy.load(Ordering::Acquire) || exchange.stop.load(Ordering::Acquire) {
        return false;
    }
    let now = web_time::Instant::now();
    {
        let mut last = lock(&exchange.last);
        if last.is_some_and(|last| now.duration_since(last) < timeout) {
            return false;
        }
        *last = Some(now);
    }
    exchange.busy.store(true, Ordering::Release);
    let mut luma = Vec::new();
    let (width, height, factor) = image.luma_plane(READ_SIDE, &mut luma);
    *lock(&exchange.job) = Some(Job {
        luma,
        width,
        height,
        factor,
        frame: (image.width, image.height),
        orientation: image.orientation,
    });
    exchange.ready.notify_one();
    true
}

/// The reader's thread: decodes each frame handed over, until stopped.
#[cfg(not(target_arch = "wasm32"))]
fn read_frames(
    exchange: &Exchange,
    formats: &[BarcodeFormat],
    speed: DetectionSpeed,
    sender: &async_channel::Sender<BarcodeCapture>,
) {
    log::info!("scanner: reading");
    loop {
        let job = {
            let mut slot = lock(&exchange.job);
            loop {
                if exchange.stop.load(Ordering::Acquire) {
                    log::info!("scanner: stopped");
                    return;
                }
                if let Some(job) = slot.take() {
                    break job;
                }
                slot = exchange.ready.wait(slot).unwrap_or_else(|e| e.into_inner());
            }
        };
        decode_job(exchange, job, formats, speed, sender);
        exchange.busy.store(false, Ordering::Release);
    }
}

fn decode_job(
    exchange: &Exchange,
    job: Job,
    formats: &[BarcodeFormat],
    speed: DetectionSpeed,
    sender: &async_channel::Sender<BarcodeCapture>,
) {
    let started = web_time::Instant::now();
    let mut barcodes = decode_luma(&job.luma, job.width, job.height, formats);
    let took = started.elapsed();
    if barcodes.is_empty() {
        log::trace!("scanner: nothing in {} ms", took.as_millis());
        return;
    }
    // The corners, from the shrunk brightness to the frame, then upright.
    let (fw, fh) = (job.frame.0 as f32, job.frame.1 as f32);
    for code in &mut barcodes {
        for corner in &mut code.corners {
            let (x, y) = (corner.0 * job.factor as f32, corner.1 * job.factor as f32);
            *corner = job.orientation.map_point(x, y, fw, fh);
        }
    }
    if speed == DetectionSpeed::NoDuplicates {
        let values: Vec<String> = barcodes
            .iter()
            .map(|b| b.raw_value.clone().unwrap_or_default())
            .collect();
        let mut reported = lock(&exchange.reported);
        if *reported == values {
            return;
        }
        *reported = values;
    }
    log::info!(
        "scanner: {} code(s) read in {} ms ({}×{} brightness)",
        barcodes.len(),
        took.as_millis(),
        job.width,
        job.height
    );
    let size = job.orientation.size(job.frame.0, job.frame.1);
    let _ = sender.try_send(BarcodeCapture { barcodes, size });
}

type DetectFn = Rc<dyn Fn(BarcodeCapture)>;
type ErrorFn = Rc<dyn Fn(CameraError)>;

/// **The scanner widget**: the camera's preview, reading the codes it sees and reporting them
/// to [`on_detect`](Self::on_detect), on the interface's thread.
///
/// It opens the camera when it enters the tree (asking for the permission the first time)
/// and closes it when it leaves. Give it a [`ScannerController`] to set the formats, the
/// speed, the camera or the torch, and to act on them while it runs.
///
/// ```no_run
/// use frus_shell::{BarcodeFormat, Scanner, ScannerController};
/// use frus_widgets::StatefulWidget;
///
/// let controller = ScannerController::new().formats([BarcodeFormat::QrCode]);
/// let _widget = Scanner::new()
///     .controller(controller.clone())
///     .on_detect(|capture| println!("{:?}", capture.barcodes[0].raw_value))
///     .on_error(|err| eprintln!("{err}"))
///     .into_widget();
/// ```
pub struct Scanner {
    controller: Option<ScannerController>,
    on_detect: Option<DetectFn>,
    on_error: Option<ErrorFn>,
    fit: BoxFit,
    background: Color,
    semantic_label: Option<String>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    /// A scanner of the back camera, its preview covering the room it is given.
    pub fn new() -> Self {
        Self {
            controller: None,
            on_detect: None,
            on_error: None,
            fit: BoxFit::Cover,
            background: Color::BLACK,
            semantic_label: None,
        }
    }

    /// The controller to run: one made for the widget otherwise.
    pub fn controller(mut self, controller: ScannerController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Called with the codes of each frame read, on the interface's thread.
    pub fn on_detect(mut self, on_detect: impl Fn(BarcodeCapture) + 'static) -> Self {
        self.on_detect = Some(Rc::new(on_detect));
        self
    }

    /// Called when the camera cannot be opened: no permission, no camera, no support here.
    pub fn on_error(mut self, on_error: impl Fn(CameraError) + 'static) -> Self {
        self.on_error = Some(Rc::new(on_error));
        self
    }

    /// How the preview fills the widget: [`BoxFit::Cover`] by default.
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// What shows before the first frame, and around a contained preview: black by default.
    pub fn background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    /// What a screen reader says the preview is.
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }
}

/// What a [`Scanner`] keeps: its controller, and whether the camera is open.
pub struct ScannerState {
    controller: ScannerController,
    /// Stopped with the widget when it made the controller.
    owned: bool,
    camera: Option<CameraController>,
    task: Option<TaskHandle<()>>,
}

impl StatefulWidget for Scanner {
    type State = ScannerState;

    fn create_state(&self) -> ScannerState {
        ScannerState {
            controller: self.controller.clone().unwrap_or_default(),
            owned: self.controller.is_none(),
            camera: None,
            task: None,
        }
    }
}

impl State for ScannerState {
    type Widget = Scanner;

    fn init_state(&mut self, cx: &StateContext<Self>) {
        let state = cx.handle();
        let controller = self.controller.clone();
        self.task = Some(state.clone().spawn(async move {
            if let Err(err) = controller.start().await {
                log::warn!("scanner: {err}");
                if let Some(on_error) = state.widget().on_error.clone() {
                    on_error(err);
                }
                return;
            }
            let camera = controller.camera();
            state.set_state(|s| s.camera = camera);
            while let Some(capture) = controller.next().await {
                if !state.is_mounted() {
                    break;
                }
                if let Some(on_detect) = state.widget().on_detect.clone() {
                    on_detect(capture);
                }
            }
        }));
    }

    fn dispose(&mut self) {
        self.task = None;
        if self.owned {
            self.controller.stop();
        }
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let widget = cx.widget();
        let mut preview = match self.camera.as_ref() {
            Some(camera) => CameraPreview::new(camera),
            None => CameraPreview::empty(),
        }
        .fit(widget.fit)
        .background(widget.background);
        if let Some(label) = widget.semantic_label.as_deref() {
            preview = preview.semantic_label(label);
        }
        Box::new(preview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A QR code of "https://frus.dev/scan" (version 2, level M), module by module.
    const CODE: [&str; 25] = [
        "#######.#..#.#....#######",
        "#.....#...##.#..#.#.....#",
        "#.###.#.###..#....#.###.#",
        "#.###.#....#.###..#.###.#",
        "#.###.#.....###.#.#.###.#",
        "#.....#.#...##....#.....#",
        "#######.#.#.#.#.#.#######",
        "...........##.###........",
        "#.#...##..#....#...#..#.#",
        ".##.##.####....#####.#.##",
        ".##..##.#..#.###.###.##.#",
        ".#.#.#.##..#..#.#.##.#...",
        "####.#####.##..##.##....#",
        ".#..##.###..##.#####...##",
        "##.#.##.#...#..##.#..##.#",
        "..###..#...##...##.###...",
        "##...###.#...########..#.",
        "........#.#..#..#...#...#",
        "#######.#.##....#.#.#...#",
        "#.....#..#...#.##...#..#.",
        "#.###.#...####.######...#",
        "#.###.#..#..#...##..#.##.",
        "#.###.#.##..###....###.##",
        "#.....#..#.##.#.#..##....",
        "#######.#.#..####....#..#",
    ];

    /// The code drawn into a `side × side` brightness, 6 pixels a module, a quiet zone round.
    fn qr(side: u32) -> Vec<u8> {
        let scale = 6;
        let offset = (side - 25 * scale) / 2;
        let mut luma = vec![235u8; (side * side) as usize];
        for (my, row) in CODE.iter().enumerate() {
            for (mx, module) in row.bytes().enumerate() {
                if module != b'#' {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let (x, y) = (
                            offset + mx as u32 * scale + dx,
                            offset + my as u32 * scale + dy,
                        );
                        luma[(y * side + x) as usize] = 20;
                    }
                }
            }
        }
        luma
    }

    #[test]
    fn a_qr_code_is_read_with_its_corners() {
        let luma = qr(200);
        let codes = decode_luma(&luma, 200, 200, &[]);
        assert_eq!(codes.len(), 1, "{codes:?}");
        assert_eq!(codes[0].format, BarcodeFormat::QrCode);
        assert_eq!(codes[0].raw_value.as_deref(), Some("https://frus.dev/scan"));
        assert!(codes[0].corners.len() >= 3);
        for &(x, y) in &codes[0].corners {
            assert!((0.0..200.0).contains(&x) && (0.0..200.0).contains(&y));
        }
        assert!(decode_luma(&luma, 200, 200, &[BarcodeFormat::Ean13]).is_empty());
    }

    #[test]
    fn a_short_or_empty_picture_reads_nothing() {
        assert!(decode_luma(&[], 0, 0, &[]).is_empty());
        assert!(decode_luma(&[0; 10], 100, 100, &[]).is_empty());
    }

    fn exchange() -> Exchange {
        Exchange {
            job: Mutex::new(None),
            ready: Condvar::new(),
            busy: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            last: Mutex::new(None),
            reported: Mutex::new(Vec::new()),
        }
    }

    fn frame(luma: &[u8], side: u32, orientation: FrameOrientation) -> CameraImage<'_> {
        // A leaked plane: the image borrows its planes.
        let planes = Box::leak(Box::new([crate::CameraPlane {
            bytes: luma,
            row_stride: side as usize,
            pixel_stride: 1,
        }]));
        CameraImage {
            width: side,
            height: side,
            format: crate::CameraImageFormat::Yuv420,
            planes,
            orientation,
        }
    }

    #[test]
    fn frames_are_handed_over_only_when_the_reader_is_free_and_the_timeout_has_passed() {
        let exchange = exchange();
        let luma = vec![0u8; 16];
        let image = frame(&luma, 4, FrameOrientation::UPRIGHT);
        let timeout = Duration::from_secs(60);
        assert!(hand_over(&exchange, &image, timeout));
        assert!(!hand_over(&exchange, &image, timeout), "busy");
        exchange.busy.store(false, Ordering::Release);
        assert!(!hand_over(&exchange, &image, timeout), "too soon");
        assert!(hand_over(&exchange, &image, Duration::ZERO));
        exchange.busy.store(false, Ordering::Release);
        exchange.stop.store(true, Ordering::Release);
        assert!(!hand_over(&exchange, &image, Duration::ZERO), "stopped");
    }

    #[test]
    fn a_code_is_reported_once_until_another_is_seen_and_its_corners_stand_upright() {
        let (sender, receiver) = async_channel::bounded(4);
        let exchange = exchange();
        let luma = qr(200);
        let job = || Job {
            luma: luma.clone(),
            width: 200,
            height: 200,
            factor: 1,
            frame: (200, 200),
            orientation: FrameOrientation::new(1, false),
        };
        decode_job(&exchange, job(), &[], DetectionSpeed::NoDuplicates, &sender);
        decode_job(&exchange, job(), &[], DetectionSpeed::NoDuplicates, &sender);
        assert_eq!(receiver.len(), 1, "a duplicate is not reported");
        let capture = receiver.try_recv().unwrap();
        assert_eq!(capture.size, (200, 200));
        // Turned a quarter: each corner is the decoded one, turned.
        let upright = decode_luma(&luma, 200, 200, &[]);
        let turned = FrameOrientation::new(1, false);
        for (got, &(x, y)) in capture.barcodes[0].corners.iter().zip(&upright[0].corners) {
            assert_eq!(*got, turned.map_point(x, y, 200.0, 200.0));
        }
        decode_job(&exchange, job(), &[], DetectionSpeed::Normal, &sender);
        assert_eq!(receiver.len(), 1, "every read in the normal speed");
    }
}
