# Milestone 816 — Hot reload: web patches link, a failed build no longer freezes the run

## Objective

Three faults in `frus run`'s hot reload, each found by driving a real session end to end:

1. **On the web, every patch panicked.** The page showed the error screen instead of the edit:
   `LinkError: WebAssembly.instantiate(): Import #6 "env"
   "_ZN12wasm_bindgen4__rt8wbg_cast17breaks_if_inlined17h…E": function import requires a
   callable`, from `subsecond::apply_patch`.
2. **A failed build froze the run.** After a compile error, `frus run` asked the compiler where
   the error was, and waited for it on its own loop: no key answered, no engine line was read
   and no patch was applied until that check ended, which could take minutes when it was cold.
3. **A patch took 20 s instead of 2** when the shell had `CARGO_INCREMENTAL=0` set.

## Root causes and fixes

### 1. The patch imports a cast descriptor the page no longer has

wasm-bindgen converts between ABI types with `wasm_bindgen::__rt::wbg_cast`, which calls a
monomorphised descriptor, `breaks_if_inlined::<From, To>`. When the wasm-bindgen CLI processes
the main module, it runs each descriptor, rewrites each `wbg_cast` to call a JS import doing
the cast, and **deletes the descriptors**.

The engine builds a patch from the changed functions and links it against the main module at
run time: every symbol the patch does not define becomes an `env` import, satisfied from the
main module's exports. It already rewrites the patch's `wbg_cast` functions to call the main
module's. Nothing calls the descriptor any more, but its import stayed, and the main module
has nothing left to satisfy it with. `WebAssembly.instantiate` refuses the module, so every
patch in a crate that uses a cast failed (any frus application does).

The fix is in the engine, in `packages/cli/src/build/patch.rs` (frus's fork of dx 0.7.10,
commit `c5438d6`, branch `frus/v0.7.10-wbg-cast`). A `wbg_cast` descriptor import is turned
into a local stub, the same way the other wasm-bindgen descriptors already were. `hot.rs`
pins that commit (`ENGINE_REV`), so the next `frus run` installs the fixed engine on its own.

An upstream fix needs the same change in dioxus's `patch.rs`: drop or stub `env` imports
whose name is a `wbg_cast` `breaks_if_inlined` descriptor before the patch is written.

### 2. The error check ran on the run's loop

The check now runs on a thread of its own (`locate::ErrorCheck`), one at a time. The loop polls
for its answer and never waits on it. An answer that is no longer wanted is dropped and its
compiler stopped (`cargo::wait_or_stopped`), in three cases:

- a patch went through, so the sources compile;
- a newer save came, so the answer is about sources that are gone;
- a newer failure started a newer check.

The check is warm. The engine noted the compiler's command for the application's crate
(`target/dx/.captured-args`), and that command runs again with only metadata asked for, so
only the application's crate is checked, in a second or two. Before the engine's first build
there is no note, and `cargo check` does the job instead.

### 3. The engine inherited `CARGO_INCREMENTAL=0`

A patch recompiles the application's crate. Without incremental compilation that took about
20 s per patch. The engine is now always started with `CARGO_INCREMENTAL=1`
(`hot::engine_command`), whatever the shell says.

### Also: `R` on the web says when it is done

The page's first request for a patch now says `started`. `frus run` counts those requests, so
"Restarted in … s" is printed when the application is up again in the page, not when the
reload was asked for.

## Evidence

Driven end to end on Windows, with the counter template app and its one-second auto counter
running throughout. The same script made the same edits each time: a label, a button text, a
type error, its fix, `R`, then one more edit.

**Web, old engine (`c5b307e`)**: all five patches failed with the `LinkError` above. The page
showed the error screen and the old label.

**Web, fixed engine (`c5438d6`)** in headless Edge:

| step | patch time | page after | `performance.timeOrigin` |
| --- | --- | --- | --- |
| label | 43.6 s (first patch) | `Count 85` | unchanged |
| button text | 8.7 s | `Count 100 … Stop auto (patched)` | unchanged |
| type error | located 2.4 s after the save; `h` answered in 0.0 s | `Count 107` | unchanged |
| fix | 6.8 s | `Count 120` | unchanged |
| `R` | restarted in 2.5 s | `Count 17` (state reset, as `R` should) | new page |
| edit after `R` | 8.5 s | `Total 32` | unchanged |

The count kept running through every patch, so state was kept. `performance.timeOrigin`
stayed the same, so each patch was applied in the same page load. The page logged no panic
and no `LinkError`.

The old engine's log, first patch:

```text
[1449.95] PAGE: panicked at …\subsecond-0.7.10\src\lib.rs:664:14:
called `Result::unwrap()` on an `Err` value: JsValue(LinkError: WebAssembly.instantiate():
Import #6 "env" "_ZN12wasm_bindgen4__rt8wbg_cast17breaks_if_inlined17hd31334fb1ad3f95cE":
function import requires a callable
```

The fixed engine's log (`PAGE:` lines come from the browser, the others from the engine's
socket):

```text
[796.05] Patched src\counter.rs in 43.6 s.
[796.16] [INFO frus_shell::hot] hot reload: patch applied
[796.17] PAGE: [INFO frus_shell::hot] hot reload: patch applied
[809.11] Patched src\counter.rs in 8.7 s.
[809.35] PAGE: [INFO frus_shell::hot] hot reload: patch applied
[864.79] Patched src\counter.rs in 8.5 s.
[865.04] PAGE: [INFO frus_shell::hot] hot reload: patch applied
```

**Desktop**, with `CARGO_INCREMENTAL=0` in the environment: patches in 2.1, 3.4, 1.8 and 2.4 s.
During the failed build, `h` answered in 0.0 s. The located error arrived 2.3 s after the save:
`src/counter.rs:70:25: error[E0308]: mismatched types: expected i32, found &str`. Before fix 3,
the same session patched in 18 to 24 s. Every patch was applied in the same process
(PID 29316). Only `R` started a new one (PID 7312), and the edit after it was patched into
that process:

```text
[541.95] Patched src\counter.rs in 2.1 s.
[541.95] [2026-11-17T13:26:11Z INFO  frus_shell::hot] hot reload: patch applied
[547.87] Patched src\counter.rs in 3.4 s.
[556.09] Patched src\counter.rs in 1.8 s.
[590.34] Patched src\counter.rs in 2.4 s.
[590.34] [2026-11-17T13:26:59Z INFO  frus_shell::hot] hot reload: patch applied
```

## Checks

fmt; clippy for the host, for `aarch64-linux-android` and for `wasm32-unknown-unknown`
(rustc 1.100.0, as CI); and `cargo test -p frus-cli`. Under rustc 1.96, clippy for Android
flags `thread_local!` in `frus-core/src/text_style.rs` as `missing_const_for_thread_local`.
Its initializer is already `const`, so this is a false positive of that clippy version, and
1.100 does not report it. frus-core is unchanged.

## Not checked

- Android hot reload (unchanged here) and hot reload on Linux and macOS (#288).
- Web patches still take 7 to 9 s, plus about 40 s for the first one. Most of that time goes
  to the engine processing the debug wasm (about 137 MB).
