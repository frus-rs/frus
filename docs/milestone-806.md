# Milestone 806 — The camera's first frames, its turn, and the memory after closing

## Objective

Milestone 798 (the camera and the code scanner) left two questions from its run on the test
phone (Huawei STK-L21, Mali-G51, Android 10):

1. **Black first frames.** The preview 6 s after opening, the preview 10 s into scanning and
   the picture's thumbnail were all black; the live picture only showed after the application
   came back from the background. The phone was being moved during that run.
2. **Memory after closing.** PSS was 146 MB while previewing and 293 MB after leaving the
   camera screen.

Find the real cause of each, fix what is frus's, and show it on the phone; and try on the
phone what milestone 798 did not: the torch, the zoom, the focus point and the other camera.

## What was found

### The black frames are the scene, not frus

The page was opened from a link, cold (the application not running) and warm, with screen
captures taken on the phone itself at 1, 3 and 6 s, and a picture taken after them. On master
and on the branch alike:

- with the lens facing something lit, the preview shows the live picture about 1 s after the
  link (the camera opens in 0.3 to 0.5 s, its first frame comes 0.5 to 1.3 s after that) and
  stays live; the picture is a real JPEG (105 to 211 KB) whose thumbnail shows the scene;
- with the phone lying on its back on the desk, the preview is black at 1, 3 and 6 s and the
  picture is a black JPEG of 48 KB. The frames do arrive (the branch logs
  `camera 0: first frame 991 ms after opening`), and turning the torch on in that same position
  lights the same preview from a mean brightness of 0 to 113 out of 255.

The picture is encoded by the camera itself, not drawn by frus, and it was black too: the
sensor saw nothing. Milestone 798's run, with the phone being moved, had the same three black
images and the same small picture; "live after the background" was the phone picked up. No
redraw was lost: a frame pushed into the texture asks the window for a frame with
`Window::request_redraw`, and every capture with light in front of the lens showed the
picture within a second of the link.

So that the next run can tell the two apart from the log alone, each camera session now says
when its first frame came (`camera 0: first frame 816 ms after opening`). No such line means
no frame; the line and a black preview mean a dark scene.

### The preview was turned for an upright phone, always

The search found a real bug in the orientation pass, which milestone 798 could not see with
the phone upright. The display's rotation was read with `Activity.getWindowManager()` on the
process's Android context, but that context is the **application**, not an activity (the
activity glue registers the `Application`): the method is not there, the call threw, and
`camera: the display's rotation is unknown (Java exception was thrown)` was logged at every
open and every resize. The rotation was taken as 0°. With the display turned to landscape,
master drew the 1280×720 frames as a 720×1280 strip in the middle of the screen, the scene on
its side, and the status line said `Preview 720×1280`.

The rotation is now read through `getSystemService("display")` and
`DisplayManager.getDisplay(DEFAULT_DISPLAY).getRotation()`, which any context answers, in a
JNI local frame of its own (the camera's thread stays attached to the VM, so its local
references would otherwise pile up a call at a time). In landscape the branch logs
`display at 90°`, says `Preview 1280×720` and fills the width.

### The memory after closing is the GPU allocator's, and it is not the camera's

`dumpsys meminfo`, the demo opened from the launcher, then the camera page from a link, left
with Back; each figure is the PSS total and, after the slash, `GL mtrack` (the GPU driver's
memory), in MB. Three runs of the same script, two of master and one of the branch:

| | master, run 1 | master, run 2 | branch |
| --- | ---: | ---: | ---: |
| home, before | 70.6 / 5.9 | 70.0 / 5.9 | 64.5 / 5.7 |
| previewing | 119.5 / 37.9 | 111.1 / 26.4 | 124.6 / 37.5 |
| left the camera page | 152.9 / 63.6 | 133.9 / 47.3 | 149.7 / 63.2 |
| 15 s later | 152.4 / 63.6 | 133.8 / 47.3 | 149.5 / 63.2 |
| after five more visits | 138.0 / 47.3 | 123.2 / 30.7 | 130.5 / 46.9 |
| back from the background, then left | 160.0 / 63.6 | — | 151.9 / 63.2 |

Nothing grows from one visit to the next; after five more visits it is lower, not higher. The
camera's own objects (device, session, requests, both image readers, the preview's texture) are
released on close: the log says `camera closed`, and the frame statistics give the renderer's
textures after leaving as `GPU memory 1.3 MB in 6 textures`, what they are at home. The native heap
is not where the growth is: 31 MB at home, 55 to 63 MB just after leaving, 31 to 54 MB after
five more visits, as the allocator hands pages back. What stays is GPU memory
the renderer no longer uses: the Vulkan allocator takes memory from the driver in blocks of
8 MB doubling to 64 MB as it holds more, and gives a block back only when nothing at all lives
in it. A page transition draws both pages into screen-sized layers; with the camera's textures
alive at the same time the blocks grow, and one small texture made during the visit keeps a
block. The figure after leaving is 14, 47 or 63 MB from one run to the next, on master and on
the branch, depending on where those small textures land.

Two changes were tried and measured, and are not in this milestone:

- polling the device a moment after the last frame, so that what the last frames dropped is
  freed without waiting for another frame: one run gave 14 MB after leaving, the next 63 MB,
  as master does; no effect that the runs can separate from chance;
- fixed blocks of 4 MB: 18 MB after leaving, also after the background; but 38 MB while
  previewing instead of 27, and a change for every application, to measure on more than one
  page (below).

## How it was tested

- `frus-shell` (`--features "camera scanner"`): its 593 tests; the orientation itself was
  already covered (`preview_orientation`, milestone 798). Reading the rotation is JNI on the
  phone, shown there.
- The phone, release APKs of `frus-demo` from master and from the branch, driven by scripts
  run on the phone itself (the USB link dropped every few minutes during the session):
  - first frames, cold and warm, lens lit and lens down, the torch as the control (above);
  - the picture: a 211 KB JPEG of the scene in the application's cache, its thumbnail shown;
  - the torch: on, the preview of a dark scene goes bright; off, dark again;
  - the zoom: three steps up read `Zoom 3.0×` and the preview is cropped; three down,
    `Zoom 1.5×`;
  - the focus point (the centre): no error, the preview stays live;
  - the other camera: `camera 1 open: … sensor at 270°`, first frame 1268 ms after opening,
    the front camera's picture upright; and back: `camera 0 open`, `Back camera`;
  - landscape (the display turned with `user_rotation`, the phone untouched): master draws the
    frames sideways in a 720×1280 strip; the branch at 1280×720, `display at 90°`;
  - memory: the table above.

The screen captures stay off the repository: they show the room, the status bar and a person.

## Size

The change is a few JNI calls swapped for others and one log line; `frus-hello` does not use
the camera feature, so its library does not contain this code.

## Not done

- **The GPU memory kept after a page transition** (14 to 63 MB after leaving the camera page,
  on master as on the branch). The allocator's block policy (`MemoryHints`, milestone 796) is
  the lever; it needs measuring across pages and on more than one phone before it changes for
  every application.
- The permission question asked while a link is being handed from a second activity to the
  running one (the demo's way of opening a link in a running process) was never shown on the
  test phone, and the permission came back `Blocked`; opening the link cold showed the
  question. Not investigated here.
- The phone's own camera application was not used to compare the dark scene; the torch and the
  camera's own JPEG were the controls.
