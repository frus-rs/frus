# Milestone 769 — Android 9 emulator: no GPU said plainly, and taps that were taken for holds

## Objective

Two bugs, both seen on an Android 9 (API 28) emulator:

1. **"Failed to create surface for any enabled backend".** A frus application closed as it
   opened on an emulator started the usual way, with that line as the only explanation.
2. **Typing into a field that already holds text replaced all of it**, with the emulator's
   keyboard: a field showing `Milk` (restored) or `a` (typed in the same process), tapped and
   given one key, held only that key (reported in milestone 762's "Not done").

## 1. No GPU frus can draw with

### Where it fails, and why

Reproduced on the `frus_api28` AVD (Android 9, x86_64, Google APIs image, emulator 36.4.9)
started with no flag beyond `-no-snapshot`:

- the emulator logs `qemu.opengles.version=131072`, and the guest reports
  `ro.opengles.version` = `131072`: **OpenGL ES 2.0**. The image has no Vulkan driver at all
  (no `vulkan.*` HAL, no `android.hardware.vulkan.*` feature);
- frus probes Vulkan first: `no Vulkan driver on this device: OpenGL ES instead`;
- wgpu's OpenGL ES backend asks EGL for a context of version 3 (`EGL_CONTEXT_MAJOR_VERSION`
  3, the minimum it supports), and the emulator's EGL refuses:
  `EGL_emulation: eglCreateContext: EGL_BAD_CONFIG: no ES 3 support`;
- with its only backend gone the instance has none, and `Instance::create_surface` returns
  `Failed to create surface for any enabled backend: {}` — the `{}` being the empty list of
  backends that were tried. frus logged it and ended the event loop (it is an error, not a
  panic, in frus 0.5).

The cause is the emulator's default, not the device: started with
`-feature GLESDynamicVersion`, the same AVD reports `qemu.opengles.version=196608` (3.0),
wgpu makes an OpenGL ES 3.0 context (`eglCreateContext: maj 3 min 0`), and the demonstration
runs (first frame logged, the Tasks page drawn). `-gpu host` and `-feature -Vulkan`, which
milestone 762 also passed, are not needed: this AVD already says `hw.gpu.mode=host`, and the
image has no Vulkan to turn off.

### Can it run on OpenGL ES 2?

Not with wgpu. Its OpenGL backend needs OpenGL ES 3.0 (or WebGL 2) for everything it is made
of — uniform buffers, vertex array objects, instancing, GLSL ES 3.00 shaders — and refuses a
2.0 context outright. A second renderer for OpenGL ES 2, or one drawing on the CPU into the
window's buffer, would be a renderer of its own, not a backend choice. The leading toolkit's
Android embedding does have such paths (its OpenGL ES renderer falls back to an ES 2 config,
and it avoids Vulkan on emulators altogether), because it ships its own renderer. For frus the
honest answer is to make the emulator offer OpenGL ES 3, and to say so when it does not.
Real devices are not concerned in practice: OpenGL ES 3.0 has been in every Android GPU of the
last decade, and frus supports Android 7 and later.

### What was done

- **The failure says why** (`frus-gpu`, `no_gpu.rs`). When the surface or the adapter cannot
  be had on Android, the renderer reads what the system says of its GPU
  (`ro.opengles.version`; `ro.kernel.qemu`/`ro.boot.qemu` for the emulator) and the outcome of
  its Vulkan probe. If the device offers OpenGL ES below 3.0 and no usable Vulkan, the error
  starts with what it offers, what frus needs, and, on the emulator, the flag:

  ```
  failed to initialise the renderer: this device offers OpenGL ES 2.0 only (no Vulkan driver
  on this device), and frus draws with Vulkan or OpenGL ES 3.0 and later. The Android emulator
  gives an Android 9 (or older) system image OpenGL ES 2.0 unless it is started with
  `-feature GLESDynamicVersion` (`frus run` and `frus emulators --launch` start it so):
  restart the emulator with that flag: Failed to create surface for any enabled backend: {}
  ```

  On a device that offers OpenGL ES 3 or Vulkan nothing is added: the cause is elsewhere and
  the survey cannot tell it. The system property is read through bionic's
  `__system_property_get`, which every Android process has; no dependency was added.
- **The emulators frus starts get OpenGL ES 3** (`frus-cli`). `frus run` (when it starts an
  emulator) and `frus emulators --launch` (the daemon's `emulator.launch`, used by the editor
  extensions) pass `-feature GLESDynamicVersion` (`devices::EMULATOR_ARGS`). Newer images have
  it on already; an emulator that does not know a feature logs that and starts all the same.
- **The getting-started guide** says the same in its Android section, for emulators started
  from Android Studio or by hand.

## 2. A tap taken for a hold

### Reproducing it

The emulator's keyboard (LatinIME) was shown with `settings put secure
show_ime_with_hard_keyboard 1`. With traces in the shell (removed afterwards) of the input
bridge's operations, the pointer events, the loop's turns and frames:

- typing into a focused field, and tapping it again before typing: nothing wrong. LatinIME's
  operations (`setComposingText`, `finishComposingText`, `setComposingRegion` over the word
  before the caret on restart) are applied as they should be; the IME side is not the cause.
- the replacement came with a **selection**: after a tap, frus itself reported
  `updateSelection(0, 3)` for a field holding `Asb`, and the next key (`commitText("n")`)
  replaced the selection, leaving `n`. The selection was the word under the finger: the tap
  had been taken for a **hold**, which in a text field selects the word (milestone 511). In a
  one-word field that is all of it.
- deterministically: `adb shell input swipe 425 890 425 890 300` (a 300 ms press, well under
  the 500 ms long press) gave, four times out of four: `down at 124.489`, `hold fired at
  125.209`, `up at 125.222 swallow true`, selection `0..1`.

### Root cause

The long press is a deadline 500 ms after the press **was read**, and it was decided in
`new_events`, at the very beginning of a turn of the event loop — before that turn read the
input waiting for it. On the emulator a turn (its frame) takes 200 ms to a second. The release
of a short tap then waits in the input while the loop draws; the next turn begins past the
deadline, fires the hold first, and reads the release after it, as the end of a hold
(swallowed).

There is a second delay, from the system: Android's input dispatcher sends no more of a
touch to a window that has not finished events it was sent half a second ago. The traces show
it: the turn after the press read only a move, and the release came one turn later still. So
even a hold decided at the end of the turn that began past the deadline (the first version of
this fix, also built and run) still selected the word.

### What was done

`decide_hold` (`frus-shell`, `app.rs`) replaces the eager poll in `new_events`:

- it runs in `about_to_wait`, **after** the turn has read its input, so a release that waited
  for the turn ends the press first;
- it fires only when the deadline had passed **when the previous turn began**. A turn that
  began after the deadline has read, and so finished, everything the system had sent; what
  that let through — the release held back behind a move — is read by the next turn, before
  the hold is decided. A hold is therefore decided one turn after the deadline: a frame on a
  phone (16 ms), more on a slow emulator.

`new_events` records when each turn began (`turn_began`, `previous_turn_began`). An idle loop
still wakes at the deadline (its control flow waits for it), and since the deadline has
passed it turns again at once, which decides the hold.

*Considered*: the events' own timestamps would be exact (Android's `MotionEvent` has them),
but winit 0.30 does not pass them on. Deciding against the clock at the end of the turn
(rather than against the turn's beginning) would fire a hold whenever a slow frame came
between the press and the release, the very bug.

### Android 10 phone

Reasoned from the code, not run: the change is in the shell's event loop, shared by every
platform; on a phone a turn is a frame of 16 ms or so, so the only difference is that a hold
is decided one frame later. The IME bridge is untouched. A check on the phone (a hold still
selects the word; tap then type keeps the text) is owed.

## How it was tested

- **Unit tests** (`frus-gpu`, `no_gpu`, 4): the property read as major/minor; the emulator with
  OpenGL ES 2 told the flag; a device with OpenGL ES 2 told its GPU is too old; nothing added
  when the device offers OpenGL ES 3 or Vulkan was tried.
- **Driver tests** (`frus-shell`, `field_drag_tests`, 2), with a new `Driver::turn(late,
  input)` that runs a turn of the loop beginning `late` after now:
  - `a_tap_whose_release_waited_behind_slow_frames_is_a_tap`: press; a turn 700 ms later that
    reads a move; a turn 1.4 s later that reads the release: a caret, no selection. It fails
    with the hold decided as a turn begins (the code before this milestone) and with it
    decided against the turn's own beginning (the first version);
  - `a_finger_still_down_after_slow_frames_is_a_hold`: the same with no release: the word is
    selected.
- **On the emulator** (`frus_api28`, x86_64 release APK of the demonstration):
  - default start: the log above, then `the activity is gone: the process ends`;
  - started with `-feature GLESDynamicVersion`: OpenGL ES 3.0 adapter, first frame, the Tasks
    page drawn;
  - with the fix and the traces: four 420–480 ms presses on a field holding `A…`, each
    followed by a `b` from the keyboard; each time the turn after the deadline read a move and
    the next turn the release (`swallow false`); the field ended `Abbbbbbbbbbbbb`, nothing
    replaced. A one-second press still selected the word and opened Cut/Copy/Paste.
  - without the traces, four 300 ms presses each followed by `b`: `Abbbb`.

## Not done

- **No OpenGL ES 2 rendering.** A device or emulator with OpenGL ES 2 only still cannot run a
  frus application; it now says so and what to change.
- **The phone.** Not run on the Android 10 phone (see above).
- **The double tap's wait** (`poll_pending_tap`) is still decided as a turn begins. A second tap
  whose press waited behind a slow frame can be read after the first was already sent as a
  single tap; nothing is selected or lost by that, so it was left.
- **`EditorInfo.initialSelStart`/`initialSelEnd`** are not filled in by the input bridge
  (they stay `-1`), where a text editor reports its selection. LatinIME recovers the caret from
  the text before it, and it was not the cause here; filling them needs the bridge's dex
  rebuilt, left for a milestone that touches the Java side.
- **Emulators started outside frus**, as by Android Studio, still need the flag added by hand
  (the guide says so); frus does not edit the AVD.
