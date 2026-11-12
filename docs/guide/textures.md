# Video, camera and other external pictures

Some pictures are not drawn by widgets: a video decoder's frames, a camera preview, a map
engine's tiles, a native view. frus shows them through an **external texture**: a producer —
any thread, any language — pushes frames into it, and a `Texture` widget shows the latest one
at its layout size.

| | Desktop | Android | Web |
| --- | --- | --- | --- |
| Pushing RGBA, BGRA, NV12, I420 frames from Rust | ✓ | ✓ | ✓ |
| A platform surface a decoder or camera draws into (`TextureSurface`) | — | ✓ | — |

## Showing a texture

`frus::textures().create()` makes a texture and gives back its producer handle; its `id()` is
what the widget shows. The widget takes the room it is offered; `fit` keeps a video's
proportions, `background` paints the bars around it.

```rust,no_run
use frus::{textures, BoxFit, Color, Texture};

let texture = textures().create();
let id = texture.id();

// The producer: here a thread, 60 frames a second.
std::thread::spawn(move || {
    let mut frame = vec![0u8; 320 * 240 * 4];
    for n in 0u32.. {
        frame.fill((n % 256) as u8);
        if texture.push_rgba(320, 240, &frame).is_err() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
});

let _view = Texture::new(id).fit(BoxFit::Contain).background(Color::BLACK);
```

What happens on a push:

- the pixels are **copied** into one of a few buffers the texture keeps and recycles, so a
  steady producer allocates nothing after its first frames;
- only the **newest** frame is kept: a producer faster than the screen has its older frames
  dropped, never queued (`texture.stats()` counts what was pushed, dropped and shown);
- the push asks the window for **one** frame, however many pushes arrive before it is drawn;
  a texture nobody pushes into costs no frame and no work at all.

Dropping the handle unregisters the texture, and the widget shows nothing (or its
`background`).

## Frames in other layouts

Decoders and cameras give YUV, many platform APIs BGRA. Each has its push, and the GPU
converts it:

```rust,no_run
use frus::{textures, YuvColorSpace, YuvPlane};

let texture = textures().create();
let (w, h) = (640u32, 360u32);
let y = vec![16u8; (w * h) as usize];
let uv = vec![128u8; (w * h / 2) as usize];

// NV12: luma, then U and V interleaved, at half size each way. Padded rows are fine: give
// each plane's row stride.
texture
    .push_nv12(w, h, YuvPlane::packed(&y, w as usize), YuvPlane::packed(&uv, w as usize), YuvColorSpace::Bt709Limited)
    .unwrap();

// Any YUV 4:2:0 described plane by plane, with pixel strides (Android's `YUV_420_888`):
let (u, v) = (YuvPlane::new(&uv, w as usize, 2), YuvPlane::new(&uv[1..], w as usize, 2));
texture
    .push_yuv420(w, h, YuvPlane::packed(&y, w as usize), u, v, YuvColorSpace::Bt601Limited)
    .unwrap();
```

`YuvColorSpace` names the matrix and the range: BT.601 for standard definition and most
cameras, BT.709 for high definition video; limited range unless the source says full.

## Android: a surface for the decoder, the camera, a native view

On Android a `TextureSurface` is a surface the platform draws into. Give it to `MediaCodec`,
Camera2, a media player or a `Canvas`; each frame they queue becomes the texture's, read on
the surface's own thread. Elsewhere `TextureSurface::new` answers `TextureError::Unsupported`.

```rust,no_run
use frus::{SurfaceFormat, Texture, TextureSurface};

// YUV for video and cameras; `SurfaceFormat::Rgba` for a canvas or a GL producer.
let surface = TextureSurface::new(1280, 720, SurfaceFormat::Yuv420).expect("Android");
let _view = Texture::new(surface.id());

// From Rust, with the NDK: `AMediaCodec_configure(codec, format, window, null, 0)`.
let _window = surface.native_window();
// For Java or Kotlin: a JNI global reference to an `android.view.Surface`, valid while
// `surface` lives (Android 8 and later).
let _java = surface.java_surface();
```

On the Java side of a plugin, the surface is an ordinary `android.view.Surface`, passed to
your method through JNI:

```java
public static void play(Context context, Surface surface, String path) throws IOException {
    MediaExtractor extractor = new MediaExtractor();
    extractor.setDataSource(path);
    MediaFormat format = extractor.getTrackFormat(0);
    extractor.selectTrack(0);
    MediaCodec codec = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME));
    codec.configure(format, surface, null, 0);   // the frames go to the frus texture
    codec.start();
    // ... the usual decoding loop: releaseOutputBuffer(index, true) shows a frame.
}
```

The same `Surface` serves `player.setVideoSurface(surface)` in a media player, a camera
capture session's output, or `surface.lockHardwareCanvas()` for a view drawn by hand. The
demo application's *External texture* screen (`crates/frus-demo/src/screens/video.rs`) plays
a clip through the NDK decoder this way.

With the `hardware-buffers` feature of `frus` (off by default, so that an application with
no video pays nothing for it), on a phone whose GPU can import Android's hardware buffers (Vulkan with
`VK_ANDROID_external_memory_android_hardware_buffer` and the sampler Y'CbCr conversion, which
most Vulkan phones have), the decoder's or the camera's frames are **not copied**: the surface
hands out GPU buffers in the producer's own format, and the GPU reads each frame where the
producer wrote it, converting YUV to RGB as it reads. Elsewhere (OpenGL ES, Android 7, an
older GPU) frames are read from the surface's CPU planes and uploaded, a copy per frame. The
choice is made when the surface is made, from what the running renderer says
(`frus::textures().hardware_buffers()`); nothing changes for the application.

A producer of its own that holds `AHardwareBuffer`s can hand them over the same way:
`texture.push_hardware_buffer(HardwareBuffer::new(ptr, w, h, release), space)`, where
`release` gives the buffer back once the GPU has read it.
