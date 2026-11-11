// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The desktop binary: calls the `run()` entry point `frus::main!` generates in the library.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus::anyhow::Result<()> {
    frus_data_example::run()
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
