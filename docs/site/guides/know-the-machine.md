---
title: Know the machine
description: Ask the probe crate for the system, the graphics card, the best lane for each kind of work, the adapters and the free disk space.
sidebar:
  order: 4
---

![The machine example: the system, the card and its memory, the best lane for embeddings, speech and video, the graphics adapters and the free disk space.](../images/machine.gif)

The `probe` crate answers what a program needs before it picks a device. It works on its own, without `embed` or the model.

```sh
cargo run --release --example machine
```

```text
Omarchy system information
Linux with AMD Vulkan
RX 9060 XT (RADV GFX1200) · 16 GB · 32 cores · 62 GB

embeddings  WebGPU
speech      Vulkan
video       VAAPI

adapter     AMD Radeon RX 9060 XT (RADV GFX1200), Vulkan, DiscreteGpu
adapter     llvmpipe (LLVM 22.1.8, 256 bits), Vulkan, Cpu
disk        382 GB free of 1999 GB
```

That is `examples/machine.rs`:

```rust
use std::path::Path;

let machine = probe::probe_machine();
println!("{}", machine.title());
println!("{}", machine.headline());
println!("{}", machine.spec_line());

println!("embeddings  {}", machine.filing.label());
println!("speech      {}", machine.listening.label());
println!("video       {}", machine.video.label());

for adapter in probe::probe_gpu().adapters {
    println!("adapter     {}, {:?}, {:?}", adapter.name, adapter.backend, adapter.device_type);
}
if let Some(disk) = probe::probe_disk(Path::new(".")) {
    println!("disk        {} GB free of {} GB", disk.free_bytes / 1_000_000_000, disk.total_bytes / 1_000_000_000);
}
```

## What it reports

`probe_machine()` returns a `Machine`:

| Field | What it holds |
| --- | --- |
| `os` | `Linux`, `MacOs`, `Windows` or `Other` |
| `release` | the system's own name, such as the distribution, when it has one |
| `vendor` | the graphics vendor: `Nvidia`, `Amd`, `Intel`, `Apple` or `None` |
| `card` | the card's name, when it is known |
| `cores`, `ram_bytes`, `vram_bytes` | the threads, the memory, and the card's own memory when it has some |
| `filing` | the best lane for embeddings |
| `listening` | the best lane for speech models |
| `video` | the best lane for video decoding and encoding |

Its `headline()`, `spec_line()` and `title()` are the three first lines above.

The lanes follow the vendor and the system:

| | Embeddings | Speech | Video |
| --- | --- | --- | --- |
| NVIDIA | WebGPU (Linux) | CUDA | NVDEC and NVENC |
| AMD on Linux | WebGPU | Vulkan | VAAPI |
| Intel on Linux | WebGPU | Vulkan | Quick Sync |
| Apple | Core ML | Metal | VideoToolbox |
| no card | the processor | the processor | the processor |

`probe_gpu()` lists every graphics adapter wgpu finds, with its backend and type, and runs once per process. `probe_disk(path)` gives the free and total bytes of the disk that holds `path`, or `None` when it can't be read. `probe_accel()` names the video accelerator alone.

[Pick a device](pick-a-device.md) turns `filing` into the device the model runs on.
