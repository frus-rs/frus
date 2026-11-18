# Milestone 818 — `LIMITATIONS.md`: every known limitation in one place

## Objective

frus's limits were written down honestly, but in a hundred places: the "Not done" and "Left"
sections of several hundred milestone notes, the open issues (with #224 grouping the limits
every self-drawing toolkit is known for), the roadmap's tagged entries and the guide pages'
platform tables. Someone deciding whether to build on frus could not find them without
`grep`, and nothing said which of them still held: many had been answered by a later
milestone while the note that recorded them stayed as it was.

The goal: one registry at the repository's root, where each limitation has a stable id, says
what a developer will run into and where, says what to do about it, and points to the evidence;
and a test that keeps the file from rotting.

## Design

### The entry

```markdown
### L-023 No automatic fallback when a present Vulkan driver misbehaves

- **Area:** rendering
- **Platforms:** Android, Linux
- **Status:** partial
- **What you hit:** …
- **Workaround:** Set `WGPU_BACKEND=gl` for such a device.
- **Evidence:** [#217](https://github.com/frus-rs/frus/issues/217), [docs/milestone-697.md](docs/milestone-697.md), …
```

A bulleted block under a heading rather than a table row: the descriptions run to several
lines, a heading gives every entry an anchor to link to, and the file stays readable as plain
text.

### Ids

`L-NNN`, three digits or more, from `L-001`. An id is never renumbered and never reused, so it
can be cited from an issue, a commit or an application's own documentation. A new entry takes
the number after the highest one in the file, wherever its area places it, so the ids are not
in order through the file once entries are added. **An entry is never deleted**: when a
limitation is lifted its status becomes `fixed in X.Y.Z` and the entry stays, so an old link
still says what happened. The test enforces the second rule the only way a file can: the ids
must run from 1 to the highest with no gap, so a deleted entry in the middle fails it.

### Statuses

`open` (nothing done yet), `partial` (part answered, the rest described in the entry) and
`fixed in X.Y.Z`, with the release that first has the fix. Nothing else: "won't fix" decisions
are design choices, and `ARCHITECTURE.md`'s "Deliberate constraints" and the roadmap's
non-goals already say them.

### Areas

The entries are grouped under thirteen sections: iOS, Android, the web, the desktops, platform
services, native views and web views, rendering, text, input, accessibility, widgets and
application structure, tooling and hot reload, packaging and size. Each entry's `Area` field
names its section's area (`platform: android`, `rendering`, …), and the desktop section's
entries say which desktop (`platform: macos`, `platform: linux`, `platform: windows`) where
only one is concerned.

## Where the entries come from

Mined, not invented:

- the 30 open issues, read with their last comments (`gh issue view`);
- the "Not done", "Not checked", "Left" and "Limits" sections of milestones 600 to 817 (the
  148 notes that have one), and of older notes where an issue or the roadmap pointed to one;
- the roadmap's 🟡 and 🔴 entries, `ARCHITECTURE.md`'s constraints, `docs/platform-integration.md`
  and the platform tables and "Not yet" sections of the guide (`docs/guide/*.md`).

**Each candidate was checked against `master` before it went in**, in the code where the code
can say it (the renderer's Vulkan probe, the shell's single window, `provide`'s global map,
`TextRun`'s fields, the router's nested-shell assertion, the hot-reload manifest, the file
dialog's platforms, the edit keys) and otherwise against the latest note or guide page on the
subject. That check removed a good share of what the notes still list as missing:

| Recorded as missing | Answered by |
| --- | --- |
| No pipeline cache (#217) | Milestone 697 (Vulkan); the entry keeps what is left |
| No OpenGL ES fallback (#217) | Milestones 681, 686 (no Vulkan, the emulator); the entry keeps the bad-driver half |
| Images decoded at full size, on the UI thread (#221) | Milestones 687, 688, 743 |
| Native views (#213), custom GPU drawing (#220) | Milestones 792, 794, 802 |
| State lost when Android kills the process (#57) | Milestone 753; the entry keeps Android 9 and older |
| A custom-scheme sign-in on Android | Milestone 801 |
| No word-processor focus rule for a tap outside a field | Milestone 750 (`TapOutside`) |
| The context menu cannot be opened from the keyboard | The Menu key and Shift+F10 open it |
| An autocomplete field 260 px wide wherever it is | Milestone 767 |
| Letter spacing | Milestone 623 |
| Web autofill turned off | Milestone 810 (`autocomplete` from the field's hint) |

Those issues are still open on GitHub; closing or updating them is not this milestone's.

## The test

`crates/frus/tests/limitations.rs`, in the facade crate, so the workspace's `cargo test` (CI's
test job) runs it. It reads `LIMITATIONS.md` and checks:

- every `### ` heading is `### L-NNN Title`, with three or more digits;
- the ids are unique and run from `L-001` to the highest with no gap;
- every entry has its six fields, none empty, a status that is `open`, `partial` or
  `fixed in X.Y.Z`, and at least one link in its evidence;
- every file the registry links to, and every `docs/milestone-NNN.md` it names, exists.

A fourth test runs the parser on a sample (a well-formed entry, a continuation line, a bad id)
so a parser that silently stopped finding anything would fail rather than pass. The test reads
the repository, so the crate's manifest leaves it out of the published package.

## How it was tested

- `cargo test -p frus --test limitations`: four tests pass.
- The guard was checked against the failures it is for, by editing a copy of the registry: an
  id used twice, an id removed from the middle, `L-12`, a status `wontfix`, an entry without
  `Workaround`, and a link to `docs/milestone-9999.md` each fail with a message that names the
  line or the id.
- `cargo +1.100.0 fmt --all`, and clippy for the host with `-D warnings`.

## Not done

- **The open issues were not updated.** Several (#213, #217, #220, #221, #57, #24, #29, #52,
  #303, #304) are largely answered by later milestones; their state on GitHub is unchanged.
- **Older notes were not read in full.** Notes before milestone 600 were read only where an
  issue, the roadmap or a guide pointed to them, so a limit recorded there and never mentioned
  again may be missing.
- **Behaviour that was never run on a platform** (most macOS paths, Linux beyond CI, phone
  browsers) is listed only where a developer will meet a missing feature or a known failure,
  plus one entry (L-015) for macOS as a whole. An exhaustive "not verified on" list would be a
  test matrix, not a registry of limits.
- **The README does not link to the registry yet**; milestone 820 is rewriting it and will.
- The roadmap's own stale entries (its status table still says 489 milestones) were not
  rewritten; it now points to the registry for what is missing today.
