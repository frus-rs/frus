# The camera and code scanning

frus opens the device's camera, shows its preview, takes pictures, hands you its frames, and
reads QR codes and barcodes from it. Two cargo features turn it on:

```toml
[dependencies]
frus = { version = "0.5", features = ["camera"] }    # the camera
# or
frus = { version = "0.5", features = ["scanner"] }   # the camera and the QR code reader
```

| | Android | Windows | Web | macOS, Linux, iOS |
| --- | --- | --- | --- | --- |
| Preview, pictures, frames | ✓ Camera2 | ✓ webcams (Media Foundation) | ✓ `getUserMedia` | `Unsupported` |
| Flash, torch, zoom, focus point | ✓ | — | — | — |
| Front and back camera | ✓ | every webcam | every camera | — |
| Reading codes | ✓ | ✓ | ✓ | — |

Where something is not available, the call answers `CameraError::Unsupported` with the
reason: nothing panics, and nothing pretends it worked.

**Android** needs the camera permission in the manifest. With `frus build` and `frus run`,
declare it once and the manifest entry is written for you:

```toml
[package.metadata.frus.permissions]
camera = "Scans the code on your ticket."
```

## Showing the camera

`available_cameras()` lists the cameras; a `CameraController` opens one; `CameraPreview`
shows it. `initialize` asks for the permission the first time, so call it from an `async`
handler or task:

```rust,no_run
use frus::{
    available_cameras, CameraController, CameraLensDirection, CameraPreview, ResolutionPreset,
    State, StateContext, StatefulWidget, Text, Widget,
};

struct CameraPage;

#[derive(Default)]
struct CameraPageState {
    camera: Option<CameraController>,
    error: Option<String>,
}

impl StatefulWidget for CameraPage {
    type State = CameraPageState;
    fn create_state(&self) -> CameraPageState {
        CameraPageState::default()
    }
}

impl State for CameraPageState {
    type Widget = CameraPage;

    fn init_state(&mut self, cx: &StateContext<Self>) {
        let state = cx.handle();
        state.clone().spawn(async move {
            let opened = async {
                let cameras = available_cameras().await?;
                let back = cameras
                    .iter()
                    .find(|c| c.lens_direction == CameraLensDirection::Back)
                    .or(cameras.first())
                    .cloned()
                    .ok_or(frus::CameraError::NotFound)?;
                let camera = CameraController::new(back, ResolutionPreset::High);
                camera.initialize().await?;
                Ok::<_, frus::CameraError>(camera)
            }
            .await;
            state.set_state(|s| match opened {
                Ok(camera) => s.camera = Some(camera),
                Err(err) => s.error = Some(err.to_string()),
            });
        });
    }

    fn dispose(&mut self) {
        // Closes the camera; dropping the last controller does too.
        if let Some(camera) = self.camera.take() {
            camera.dispose();
        }
    }

    fn build(&self, _cx: &StateContext<Self>) -> Box<dyn Widget> {
        match (&self.camera, &self.error) {
            (Some(camera), _) => Box::new(CameraPreview::new(camera)),
            (None, Some(error)) => Box::new(Text::new(error.clone())),
            (None, None) => Box::new(CameraPreview::empty()),
        }
    }
}
```

The preview takes the room it is given and shows the whole frame, upright, with black bars
around it; `.fit(BoxFit::Cover)` fills the room and crops instead, `.background(color)`
changes the bars. On a phone the frame is turned on the GPU to follow the sensor's mounting
and the screen's rotation, and the front camera is mirrored, as a mirror shows a face.

`ResolutionPreset` picks the size: `Low` (about 320×240), `Medium` (720×480), `High`
(1280×720, the default), `VeryHigh` (1920×1080), `UltraHigh` (3840×2160) or `Max`. Pictures
are taken at the size of the preset too; use `Max` for the sensor's full resolution.

## Pictures, flash, zoom and focus

```rust,no_run
use frus::{CameraController, FlashMode};

async fn shoot(camera: &CameraController) -> Result<(), frus::CameraError> {
    camera.set_flash_mode(FlashMode::Auto)?;   // Off, Auto, Always, Torch
    camera.set_zoom_level(2.0)?;               // between min_zoom_level() and max_zoom_level()
    camera.set_focus_point(Some((0.5, 0.5)))?; // in 0..1 of the preview; None: the whole picture
    let picture = camera.take_picture().await?;
    // JPEG bytes, and where they were saved in the application's cache folder.
    println!("{} bytes at {:?}", picture.bytes.len(), picture.path);
    Ok(())
}
```

On Android the JPEG is the camera's own, turned upright by its EXIF orientation. To show it
on the screen, decode it (`Image::memory`, or `frus::image_cache` for a file) as any picture.

`set_description(other)` switches to another camera (the front one) and keeps the preview;
`pause_preview()` and `resume_preview()` close it and open it again. You do not need either
for the application's lifecycle: **the camera closes by itself when the application goes to
the background, and opens again when it comes back.**

## The frames themselves

`start_image_stream` hands every frame to a closure **on the camera's thread**, borrowed for
the time of the call: copy what you keep, and return quickly, or frames are dropped. The
frame says its layout (`Yuv420` on Android, `Nv12` on Windows, `Rgba8` on the web), its
planes with their strides, and how it must turn to stand upright.

```rust,no_run
use frus::CameraController;

fn watch_brightness(camera: &CameraController) -> Result<(), frus::CameraError> {
    camera.start_image_stream(|image| {
        let mut luma = Vec::new();
        // The brightness, shrunk until no side is longer than 160 pixels.
        let (w, h, _factor) = image.luma_plane(160, &mut luma);
        let mean = luma.iter().map(|&v| v as u64).sum::<u64>() / (w as u64 * h as u64).max(1);
        println!("brightness {mean}");
    })
}
```

`stop_image_stream()` stops it. While no stream listens, the frames are not even looked at.

## Reading QR codes and barcodes

The `scanner` feature adds the `Scanner` widget: the camera's preview, reading the codes it
sees and calling `on_detect` with them, on the interface's thread:

```rust,no_run
use frus::{Scanner, StatefulWidget, Widget};

fn scan(on_code: impl Fn(String) + 'static) -> Box<dyn Widget> {
    Box::new(
        Scanner::new()
            .on_detect(move |capture| {
                for code in &capture.barcodes {
                    // The format, the text, and the corners in the preview's pixels.
                    if let Some(value) = &code.raw_value {
                        on_code(value.clone());
                    }
                }
            })
            .on_error(|err| eprintln!("no scanner: {err}"))
            .into_widget(),
    )
}
```

The widget opens the camera when it enters the tree and closes it when it leaves. A
`ScannerController` given with `.controller(..)` sets what it reads and lets you act while it
runs:

```rust,no_run
use std::time::Duration;
use frus::{BarcodeFormat, CameraLensDirection, DetectionSpeed, ScannerController};

let controller = ScannerController::new()
    .facing(CameraLensDirection::Back)
    .formats([BarcodeFormat::QrCode])
    .detection_speed(DetectionSpeed::NoDuplicates) // a code once, until another is seen
    .detection_timeout(Duration::from_millis(250)) // the least time between two reads
    .torch_enabled(false);
// Later: controller.set_torch(true), controller.switch_camera().await, controller.stop().
# let _ = controller;
```

`DetectionSpeed::Normal` reports every read (at most once per timeout) and `Unrestricted`
reads every frame the reader is free for. `BarcodeCapture::size` is the size of the upright
frame the corners are given in, for drawing a box over the code.

The `scanner` feature reads **QR codes**. `scanner-all-formats` adds every format the reader
knows — EAN-8 and EAN-13, UPC-A and UPC-E, Code 39, 93 and 128, ITF, Codabar, Data Matrix,
PDF417, Aztec — for about a megabyte more in a release build.

`decode_luma(&brightness, width, height, &formats)` reads the codes of any picture you have
the brightness of, a file's or a screenshot's.

### What scanning costs

The scanner is built to weigh little on a phone:

- its camera opens at `ResolutionPreset::Medium`, with no picture stream;
- a frame is read only when the reader is idle **and** the detection timeout has passed: its
  brightness (at most 720 pixels a side) is copied and decoded on a thread of the scanner's
  own, so neither the camera nor the interface waits for a decode;
- the reader is pure Rust ([rxing](https://crates.io/crates/rxing)): nothing to install,
  and the same on every platform;
- before `start` and after `stop` nothing runs: no thread, no camera, no texture.

On the web there is no thread to decode on: the frame is decoded where it arrives, still no
more often than the timeout says.

## Platform notes

- **Android**: Camera2 through the NDK, looked up when the first camera is listed, so an
  application that never opens one loads nothing. The preview streams into a
  `TextureSurface` (see [textures](textures.md)). Pictures in `Auto` or `Always` flash wait
  for the exposure to measure with the flash first (about 0.4 s).
- **Windows**: the webcams Media Foundation lists, NV12 at the size closest to the preset. A
  picture is the next frame, encoded as JPEG. Webcams answer `Unsupported` for the flash, the
  zoom and the focus point.
- **The web**: `getUserMedia`, which browsers allow on secure pages only (`https`, or
  `localhost` while developing). A picture is the latest frame, encoded by the browser.
