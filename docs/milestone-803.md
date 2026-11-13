# Milestone 803 — Back under the Android size budget

## Objective

The `Size budget (Android)` job measures `frus-hello`, the counter, built for
aarch64-linux-android with the release profile, against `ci/size-budget.toml`
(14,500,000 bytes). On master `f9d99844` it passed at 14,496,648 bytes in the morning and
failed at 14,501,512 bytes the same evening, with nothing in the repository changed between
the two runs: the job installs the latest stable rustc, and rustc 1.100.0 (released
2026-11-09) makes the same code about 42 KB larger than 1.96.1. Every open pull request
failed the job from then on.

The budget is not raised. The library has to shrink instead, by at least 6 KB, so that the
next small change has room, without taking anything away from what a default application
does.

## Finding what to take out

The library was built once with `strip = false` (`CARGO_PROFILE_RELEASE_STRIP=false`), its
symbols listed with the NDK's `llvm-nm -S --size-sort -C`, and their sizes summed by crate:

| Crate | Bytes of code and data |
| --- | ---: |
| naga | 1,146,000 |
| wgpu_core | 641,000 |
| frus_widgets | 523,000 |
| the text shaper | 340,000 |
| … | |
| **regex_automata** | **241,000** |
| **regex_syntax** | **127,000** |
| **aho_corasick** | **76,000** |

A counter has no use for a regular-expression engine. `cargo tree --target
aarch64-linux-android -i regex` names where it comes from:

```text
regex
└── env_filter
    └── android_logger
        └── frus-shell
```

`android_logger` turns on `env_filter`'s `regex` feature by default. With it, the message
part of a filter directive (`crate=level/pattern`) is a regular expression; without it, that
part is a plain substring, and every other part of the syntax is read the same way (the
difference in `env_filter` 0.1 is one type, `FilterOp`, a `Regex` or a `String`). The filter
frus hands logcat's logger is `logging::default_filter`: `info,crate=warn,…,app=debug`, never
with a `/pattern`, and nothing else can reach it on Android (there is no `RUST_LOG` there).
The engine was in every Android application and never ran.

## The change

`crates/frus-shell/Cargo.toml`:

```toml
android_logger = { version = "0.14", default-features = false }
```

`regex`, `regex-automata`, `regex-syntax` and `aho-corasick` leave the Android build (the
desktop keeps `regex` through `env_logger`, whose `RUST_LOG` is the user's to write). A test,
`the_default_filter_needs_no_pattern_engine`, holds the default filter to directives without
a pattern, the only ones that read the same without the engine.

`ci/size-budget.toml` gains one line in its history of measurements; `budget_bytes` is
unchanged.

## Measurements

`cargo build --release -p frus-hello --lib --target aarch64-linux-android`, NDK
26.3.11579264 (r26d, the version CI installs), stripped `libfrus_hello.so`, before and after
on the same toolchain:

| rustc | master `f9d99844` | this branch | Change |
| --- | ---: | ---: | ---: |
| 1.100.0 (CI's stable) | 14,499,592 | 13,793,112 | −706,480 |
| 1.96.1 | 14,457,096 | 13,749,336 | −707,760 |

The remaining difference between the local 1.100.0 number and CI's (14,501,512 on master) is
about 2 KB and comes from the build environment; CI's own measurement of this branch is in
the pull request. With the budget at 14,500,000, about 700 KB of room is back, the same room
as before the 784 dictionaries, at no cost to speed: the release profile is untouched
(`opt-level = 3`, fat LTO, one codegen unit).

## How it was tested

- The size, built before and after with both toolchains, as above.
- `cargo tree` on the Android target lists no `regex`, `regex-automata`, `regex-syntax` or
  `aho-corasick` after the change.
- `env_filter` 0.1.4's source: the `regex` feature switches only the message matcher.
- The new unit test, and the workspace's checks: `cargo fmt`, clippy with `-D warnings` for
  the workspace and for `frus-shell` on aarch64-linux-android, the workspace tests, the strict
  rustdoc build.

## Not done

- Not run on the phone: the change selects how `env_filter` matches a message pattern, a
  path the default filter never takes, and the logger is otherwise the same code. The logs
  reaching logcat were not looked at on a device for this milestone.
- The other large items in the symbol list were looked at and left: the WGSL front end and
  SPIR-V/GLSL back ends of naga are how shaders reach the GPU, and the backtrace symbolizer
  (`gimli`, about 40 KB) is the standard library's default panic report, which a stable
  toolchain cannot drop. Each would be a smaller win at a real cost.
