---
title: Ship a binary
description: Build a program that embeds on the GPU and carry the WebGPU library it needs alongside it.
sidebar:
  order: 6
---

On Linux the GPU build links ONNX Runtime's `libwebgpu_dawn.so` (12 MB). The `ort` crate places it next to your binary in `target/<profile>/`, so `cargo run` and `cargo test` find it. A binary you ship carries it alongside.

## Linux

Copy the library next to the binary and let the binary look there, with an rpath of `$ORIGIN`. In your binary crate's `build.rs`:

```rust
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
}
```

Then ship both files together:

```sh
cargo build --release
mkdir -p dist
cp target/release/<your-binary> target/release/libwebgpu_dawn.so dist/
```

Without the rpath, point the loader at the folder instead:

```sh
LD_LIBRARY_PATH=dist dist/<your-binary>
```

A binary that can't find the library stops at start:

```text
error while loading shared libraries: libwebgpu_dawn.so: cannot open shared object file: No such file or directory
```

A CPU-only build, with the default `vulkan` feature turned off (see [Pick a device](pick-a-device.md)), doesn't link it.

## The model

No build carries the model. Ship its six files, or download them on the first run as [Get the model](get-the-model.md) shows; the Gemma Terms of Use cover them.

The search example builds to 47 MB in release, ONNX Runtime included.
