# Milestone 772 — A debug run is smooth: applications optimise their dependencies

## Objective

A developer running the Forge example with `frus run` found every page change slow and asked
whether frus or the application was at fault.

## What was measured

A test drove the real application with `Tester` on a 402 by 874 surface: signed in, then the
four bottom tabs tapped in turn, three rounds, each tap followed by frames until the page
transition ended. The same test was built twice: as `frus run` builds it (Cargo's `dev`
profile, nothing optimised), and with `profile.dev.package."*".opt-level = 3`.

| | Debug as generated | Dependencies optimised |
|---|---|---|
| First page | 2.94 s | 0.26 s |
| To the dashboard | ~1.2 s | ~0.28 s |
| To the projects | ~1.1 s | ~0.31 s |
| To notifications, settings | ~0.2 s | ~0.15 s |
| One frame of the dashboard | 31 ms | 4.3 ms |

These are CPU times only (the tester has no GPU); they compare the two builds, they are not
on-screen times. At 31 ms a frame the dashboard runs below 30 fps, and a page transition draws
two pages at once.

## Cause

frus's own workspace optimises its dependencies in `dev` (`[profile.dev.package."*"]
opt-level = 3`), which is why the demo was smooth. An application is its own workspace, and
neither the `frus create` template nor any of the examples declared it: wgpu, the text
shaper, lyon's tessellation, the SVG code and frus itself ran unoptimised in every
`frus run`. The application's code was not the cause: Forge's heaviest pages are its fullest
ones, well inside a frame once the dependencies are optimised.

## What changed

- The template's `Cargo.toml` declares `[profile.dev.package."*"] opt-level = 3`, explained
  under a new "While you develop" heading. The application's own crate is left at Cargo's
  default: it rebuilds quickly and hot reload patches it as before. The dependencies take
  longer to build once, then they are cached.
- The eight examples in `frus-examples` declare the same.
- A test checks the rendered manifest keeps the section, and does not set `[profile.dev]`
  itself.

## Alternatives weighed

- **`opt-level = 1` for the application too**, as frus's workspace does: faster code in the
  application, at the cost of slower rebuilds of the one crate a developer edits all day, and
  a change to the code hot reload patches. The measured gain came from the dependencies alone,
  so the application keeps the default; a developer can add it.
- **Passing the setting from `frus run`** (`--config`): it would cover existing applications
  without an edit, but a plain `cargo run` or an editor's own build would still be slow, and
  the setting would be hidden. The manifest is where Cargo looks and where a developer reads.

## Existing applications

An application made before this milestone gets the setting by adding the two lines to its
`Cargo.toml`. `frus create` in an existing application restores only missing files, so it
does not add them.
