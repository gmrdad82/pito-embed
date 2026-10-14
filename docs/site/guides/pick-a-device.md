---
title: Pick a device
description: Run the model on the GPU when there is one, on the CPU otherwise, and find out why when the card refuses.
sidebar:
  order: 3
---

`Embedder::load` takes the folder of the model and an `EmbedDevice`:

| `EmbedDevice` | What it runs on | Built on |
| --- | --- | --- |
| `WebGpu` | ONNX Runtime's WebGPU provider over Vulkan | Linux, with the default `vulkan` feature |
| `CoreMl` | CoreML | macOS |
| `Cpu` | ONNX Runtime's CPU provider | everywhere |

`EmbedDevice::compiled()` names the GPU device this build carries, and `label()` gives a name to print: `Vulkan`, `CoreML` or `the processor`.

## Let the probe choose

`probe::probe_machine().filing` is the best lane for embeddings on this machine: WebGPU on Linux and Core ML on macOS when there is a graphics card, the processor when there isn't. Turn it into a device and fall back to the CPU for any lane that isn't one:

```rust
use embed::{EmbedDevice, Embedder};

let lane = probe::probe_machine().filing;
let device = EmbedDevice::try_from(lane).unwrap_or(EmbedDevice::Cpu);
let embedder = Embedder::load(&model_dir, device)?;
println!("running on {}", embedder.device_in_use().label());
```

## When the card refuses

If the GPU provider can't start, or you ask for a GPU device this build doesn't carry, `Embedder::load` loads the model on the CPU instead and keeps going. `device_in_use()` says where it runs and `fall_off_reason()` says why:

```rust
if let Some(reason) = embedder.fall_off_reason() {
    println!("the card refused, so it runs on the processor: {reason}");
}
```

The search example prints exactly that line when it happens. A missing model file is never turned into a CPU load: it stays `EmbedError::ModelMissing`.

## Only the CPU

To skip the GPU, pass `EmbedDevice::Cpu`. To leave the GPU provider out of the build altogether, which also drops the WebGPU library from it, turn off the default features:

```toml
embed = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v@VERSION@", default-features = false }
```

## Which is faster

Batches are where the card pays: 256 lines run 6.3× faster on Vulkan than on the CPU, one query 1.7×. Loading the model and the first query cost more on the GPU, since its shaders are set up then, so load once and keep the `Embedder`. [Measure the speed](measure-speed.md) has the numbers and how to get yours.

The macOS CoreML path builds and passes CI on the CPU; it is untested on Apple hardware here.
