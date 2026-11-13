# Milestone 798 — The camera and code scanning

## Objective

An application must be able to open the device's camera, show its preview, take a picture,
read its frames, and read QR codes and barcodes from it, with a few lines and without a
plugin of its own: the two packages every application of the leading toolkit reaches for
(the camera, and a barcode scanner on top of it), with their shapes. It builds on the external
textures of milestone 793 and the texture surface of milestone 795.

## API

```rust
use frus::{available_cameras, CameraController, CameraPreview, ResolutionPreset, FlashMode};

let cameras = available_cameras().await?;            // lens direction, sensor orientation
let camera = CameraController::new(cameras[0].clone(), ResolutionPreset::High);
camera.initialize().await?;                          // asks for the permission the first time
CameraPreview::new(&camera).fit(BoxFit::Cover);      // the widget
let picture = camera.take_picture().await?;          // JPEG bytes, and a file in the cache
camera.start_image_stream(|image| { .. })?;          // planes, borrowed on the camera's thread
camera.set_flash_mode(FlashMode::Torch)?;
camera.set_zoom_level(2.0)?; camera.set_focus_point(Some((0.5, 0.5)))?;
camera.set_description(other).await?;                // the other camera, same preview
camera.pause_preview(); camera.resume_preview().await?; camera.dispose();

use frus::{Scanner, ScannerController, BarcodeFormat, DetectionSpeed, decode_luma};
Scanner::new()
    .controller(ScannerController::new().formats([BarcodeFormat::QrCode]))
    .on_detect(|capture| { for code in &capture.barcodes { .. } })
    .on_error(|err| ..);
decode_luma(&brightness, width, height, &[]);        // any picture's codes
```

Features of `frus` (and `frus-shell`), off by default: `camera`; `scanner` (implies `camera`,
QR codes); `scanner-all-formats` (every format the reader knows). New in `frus-core`:
`FrameOrientation` and `ExternalTexture::set_orientation`. New in `frus-shell`:
`TextureSurface::set_frame_observer`, `SurfaceFrame`, `FrameObserver`,
`TextureSurface::readable_with_texture` and `TextureSurface::into_texture`. Guide:
`docs/guide/camera.md`, compiled as doctests.

## Design

### Turning frames on the GPU

A phone's sensor is mounted sideways: the back camera's frames must turn a quarter to stand
upright, the front camera's must also be mirrored, and both change with the display's
rotation. Turning them on the CPU would cost a copy of every frame. Instead an external
texture carries an **orientation** (`FrameOrientation`: quarter turns and a mirror), stamped
on every frame pushed after `set_orientation`, and the GPU conversion pass that already turns
YUV into RGB (milestone 793) samples the planes through the turn: a turned frame is converted
into a texture of the size it is drawn at. An upright RGBA frame still skips the pass; a
turned RGBA frame goes through it. `FrameInfo` gives the size as drawn, so layout sees the
upright size. The turn costs nothing measurable on the GPU (one more matrix in the shader).

The pass's uniforms grew by the turn; a WGSL field named `target`, a reserved word, had to be
renamed. The six GPU tests of `frus-test`'s `external_texture` check each turn, the mirror
and the drawn size, rendered on D3D12.

### Android: Camera2 through the NDK

`libcamera2ndk.so` exists since Android 7; it is reached with `dlsym` the first time a camera
is listed, as the texture surface reaches the image reader, so an application that never
opens a camera loads nothing and still starts on Android 7. The alternative, the Java
`camera2` package through JNI with a dex of our own (as video did in milestone 797), was not
needed: the NDK has everything a preview, a still and the controls take, without a Java
callback per frame.

- **The preview** is a repeating request into a `TextureSurface` (an image reader of YUV
  4:2:0). Its frames become the texture's, and a frame observer hands them to the image
  stream and the scanner on the reader's thread. The texture's orientation is worked out
  from the sensor's orientation, the lens's direction and the display's rotation, and again
  when the window is resized (a turn of the phone).
- **The size**: among the YUV output sizes of the stream configurations, the largest that
  fits in the preset's target (`Low` 320×240 … `Max`), long side against long side, else the
  smallest larger one; the still's size keeps the preview's shape.
- **A picture** is a single capture into a second image reader, of JPEG images, with
  `JPEG_ORIENTATION` set so the picture stands upright, written to the application's cache.
  With the flash in `Auto` or `Always`, a precapture metering runs first.
- **Flash, torch, zoom, focus**: entries of the requests (`FLASH_MODE`, `AE_MODE`,
  `SCALER_CROP_REGION` within `MAX_DIGITAL_ZOOM`, `AF_REGIONS`/`AE_REGIONS` with an
  `AF_TRIGGER`); changing one sends the repeating request again. A focus point given in the
  preview's upright coordinates is mapped back to the sensor through the same orientation.
- **The lifecycle**: the shell's lifecycle hook closes every open camera when the
  application goes to the background (another application may want it, and Android takes it
  anyway) and opens it again, on a thread of its own, when it comes back. The preview texture
  stays, so a `CameraPreview` keeps its id and shows the last frame meanwhile.
- **The permission**: `initialize` asks for `android.permission.CAMERA` through the
  permission module of milestone 730; `frus build` writes the manifest entry from
  `[package.metadata.frus.permissions] camera = ".."`.

The camera's surface is always made **readable**. Master's milestone 800 lets a texture
surface hand its frames to the GPU as hardware buffers, imported without a copy; such
buffers (`AIMAGE_FORMAT_PRIVATE`) cannot be read by the CPU, and the image stream and the
scanner read the planes. `TextureSurface::readable_with_texture` makes a surface that always
copies planes, whatever the renderer imports; the camera uses it. A camera that shows a
preview with no stream and no scanner could use hardware buffers instead (below).

### Windows: Media Foundation

`MFEnumDeviceSources` lists the webcams; a thread of the session's own opens one with a
source reader asking for NV12 at the native type closest to the preset, reads the frames one
after the other, pushes each into the texture (converted on the GPU) and hands it to the
image stream. A picture is the next frame, converted to RGB and encoded as JPEG with `image`
(already in the tree through `frus-image`). The thread ends with the session, at the latest
one frame after it is asked to. Webcams answer `Unsupported` for the flash, the zoom and the
focus point: Media Foundation's `IAMCameraControl` reaches them on some devices only, and
none of the machines at hand had them.

### The web: `getUserMedia`

The stream plays in a `<video>` that is never put on the page; each animation frame where
the video has a new frame draws it into a canvas and pushes the canvas's pixels (RGBA) into
the texture and the image stream. A picture is the canvas encoded as JPEG by the browser
(`toDataURL("image/jpeg")`). Secure pages only (`https`, `localhost`), as browsers require.

### macOS, Linux, iOS

Everything answers `CameraError::Unsupported` with the reason. AVFoundation and PipeWire /
V4L2 are the paths; neither was in reach of this milestone.

### The scanner

The reader is [rxing](https://crates.io/crates/rxing) 0.9, a pure-Rust port of ZXing. Measured
on the host (release, LTO, a 640×480 QR code with noise):

| | binary size | decode |
| --- | --- | --- |
| rqrr 0.10 (QR only) | +260 KB | 22–74 ms |
| rxing, `QRCodeReader` only | +475 KB | 6–9 ms |
| rxing, `MultiFormatReader` | +1.55 MB | 8–25 ms |

rqrr is smaller but three to ten times slower and reads QR codes only. rxing's QR reader is
what `scanner` compiles (features `decoders`, `qrcode`; no `chrono-tz`, no image crate);
`scanner-all-formats` adds `full_barcode_format_support` and the multi-format reader.

How it stays light on a phone:

- the camera opens at `ResolutionPreset::Medium`, with no picture stream;
- a frame is read only when the reader is idle **and** the detection timeout (250 ms by
  default) has passed since the last read: its brightness, shrunk to at most 720 pixels a
  side by a whole factor, is copied and sent to a thread of the scanner's own, which decodes
  it; the camera's thread never waits for a decode, and every other frame is not looked at;
- results go to the interface's thread through a channel woken by a task (milestone 714),
  and `on_detect` runs there; `DetectionSpeed::NoDuplicates` reports a code once until
  another is seen, `Normal` every read, `Unrestricted` every frame the reader is free for;
- the corners are given in the upright frame's pixels, with that frame's size, to draw a box
  over the code;
- before `start` and after `stop`: no thread, no camera, no texture.

On the web there is no thread to decode on: the frame is decoded where it arrives, still no
more often than the timeout says.

### Cost when no camera is open

Nothing: no thread, no texture, no timer, no symbol looked up. The lifecycle hook walks an
empty list. Without the features, nothing is compiled in.

## How it was tested

### Unit tests (`cargo test -p frus-shell --features "camera scanner" --lib`)

- The orientation: a back camera on a phone held upright turns a quarter; a front camera is
  mirrored and turns the other way with the display; a point of the preview is found back on
  the sensor (the focus point).
- Frames: the brightness plane is shrunk by a whole factor, for YUV and RGBA frames; an image
  stream is only fed while it listens; the size closest to each preset is chosen.
- The widgets: a preview of no camera paints its background; a controller does nothing
  before it is initialized and answers `NotInitialized`.
- The scanner: a QR code generated by rxing's encoder is read back from its brightness with
  its corners; a short or empty picture reads nothing; frames are handed over only when the
  reader is free and the timeout has passed; with `NoDuplicates` a code is reported once until
  another is seen, its corners turned upright with the frame.
- `frus-core`: an orientation turns the frames pushed after it and the drawn size; `frus-gpu`:
  turned frames are converted into a texture of their drawn size; `frus-test`
  (`external_texture`, rendered on D3D12): turned frames land where their orientation puts
  them, and a turned frame is drawn at its turned size.
- The guide compiles as doctests (`cargo test -p frus --features "camera scanner" --doc`).

### Windows, a real webcam

A check program (not in the tree; it prints no pixels) opened the machine's USB webcam
through Media Foundation: listed as an external camera, opened at 1280×720 NV12 in 638 ms,
85 frames in 5 s to the image stream (17 fps, the webcam's rate in that light), a picture of
25.8 KB, a valid JPEG, in 104 ms, saved to the cache. The torch answered `Unsupported`, and
so does a zoom since the check found `set_zoom_level(2.0)` quietly clamped to 1 on a camera
without a zoom. Closing ended the thread and removed the texture. The scanner opened the
same webcam at 640×480 in 412 ms, read, and stopped with its camera closed.

### Android, the test phone

A release APK of `frus-demo` on the test phone (Mali-G51, Android 10), opened at
`frusdemo://camera`. A script drove it with `adb`. The USB link dropped twice during the
run, so the background step was not measured.

- **Preview:** the back camera opened at 720×1280 and the frames stood upright. The app used
  about 25% of a core (`top`) while previewing, with 146 MB PSS.
- **Scanner:** a QR code shown on the PC's screen was read as
  `QrCode: https://frus.dev/scan-j798`, with the live picture behind it. CPU stayed at
  about 25% while scanning.
- **Picture:** "Take a picture" saved a 105 KB JPEG to the app's cache and showed its
  thumbnail.
- **Leaving the screen:** the log shows `scanner: stopped`, then `camera closed`, and the CPU
  dropped to 0.0%.
- **Open questions:** 1) The first preview screenshot (6 s after opening), the one taken
  10 s into scanning, and the picture's thumbnail are all black. The live picture showed up
  only after the app came back from the background. The phone was being moved during the
  run (the USB link dropped), so this run can't say whether the lens was covered or the first
  frames really are black. A rerun with the phone held still is owed. 2) After the camera
  screen was closed, PSS was 293 MB (146 MB while previewing) and RSS 340 MB (196 MB). That
  growth was not explained in this run.
- Not exercised on the phone: the torch, zoom, focus point and switching cameras.

### Size

`frus-hello`, release, aarch64 Android, built from master and from the branch merged with it
on the same machine and toolchain (rustc 1.96.1):

| | bytes |
|---|---|
| master | 13,732,568 |
| branch | 13,732,440 |
| difference | -128 |

The first branch build was 1,152 bytes over master: the colour conversion shader carried its
explanation as WGSL comments, which ship inside the library. They are Rust docs on the
shader's module now.

`frus-demo`, which turns on `camera` and `scanner`: 18,874,600 bytes on master, 19,539,112 on
the branch (+664,512 bytes: the QR reader and the camera's three backends' Android part).

## Not done

- Zero-copy camera preview: the camera's surface is always readable (above). A preview with
  no stream and no scanner could take hardware buffers and switch to a readable surface when
  a stream starts; the session would have to be rebuilt for it.
- macOS, Linux and iOS cameras.
- The flash, the zoom and the focus point on webcams and in browsers (`IAMCameraControl`,
  `MediaStreamTrack.applyConstraints` with `zoom` and `torch`, where the device has them).
- Decoding on the web off the page's thread (a worker).
