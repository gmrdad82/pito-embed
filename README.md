<p align="center"><img src="docs/images/search.gif" width="800" alt="A terminal running the search example twice. search a hidden area loads EmbeddingGemma-300m on Vulkan in about 0.9 seconds, embeds eight gaming lines and the query in under 90 ms, 768 dimensions each, and ranks found a secret room behind the waterfall first at 0.567; search great music ranks the soundtrack in this level is incredible first at 0.407."></p>

# pito-embed

[![CI](https://github.com/gmrdad82/pito-embed/actions/workflows/ci.yml/badge.svg)](https://github.com/gmrdad82/pito-embed/actions/workflows/ci.yml) [![Version](https://img.shields.io/github/v/tag/gmrdad82/pito-embed)](https://github.com/gmrdad82/pito-embed/tags)

pito-embed turns text into embeddings on your own machine: 768-number vectors where lines that mean the same thing sit close together, so you can search by meaning instead of by words. It runs Google's EmbeddingGemma-300m through [fastembed-rs](https://github.com/Anush008/fastembed-rs) and ONNX Runtime, on the GPU when there is one and on the CPU otherwise, with no network at inference time. It is part of [PITO](https://pitomd.com).

Works on Linux and macOS (Windows through WSL).

## What it is

A Cargo workspace of two Rust crates:

- **`embed`**, at the root: the embedder. `Embedder::load` opens the model from a folder on the device you ask for, `embed_query` and `embed_documents` return unit-length vectors (so a dot product is the cosine similarity), with EmbeddingGemma's query and document prompts applied for you. When the GPU refuses the model it falls back to the CPU and tells you why. `embed::catalog()` lists the six model files with their download URLs, sizes and sha256.
- **`probe`**, in `crates/probe`: facts about the machine: the GPU adapters (through wgpu), free disk space, the hardware video accelerator and a summary of the machine with the best execution lane for each kind of work. `embed` reads it to pick a device.

The devices:

| OS | GPU | Fallback |
| --- | --- | --- |
| Linux | ONNX Runtime's WebGPU provider over Vulkan (the default `vulkan` feature) | CPU |
| macOS | CoreML | CPU |

On the machine in the clip (an AMD card on Linux, Vulkan), the model loads in about 0.9 s and eight short lines plus a query embed in 45 to 90 ms.

## Install

pito-embed isn't on crates.io; add it from this repository, pinned to a tag. Both crates come from the one repository, and a project that uses both pins them at the same tag:

```toml
[dependencies]
embed = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v0.3.0" }
probe = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v0.3.0" }
```

It needs Rust 1.97 or newer. The build downloads ONNX Runtime's prebuilt libraries through the [`ort`](https://github.com/pykeio/ort) crate.

- **Linux:** a C toolchain, `pkg-config` and OpenSSL's headers (`build-essential pkg-config libssl-dev` on Debian and Ubuntu). The GPU build links ONNX Runtime's `libwebgpu_dawn.so`, which `ort` places next to your binary in `target/<profile>/`; `cargo run` and `cargo test` find it, and a binary you ship carries it alongside (with an rpath of `$ORIGIN`, or `LD_LIBRARY_PATH`). For a CPU-only build without it, turn off the default features: `embed = { …, default-features = false }`.
- **macOS:** Xcode's command line tools. CoreML comes with the system.
- **Windows:** run it under WSL, as on Linux.

## Use

```rust
use embed::{EmbedDevice, Embedder};

let lane = probe::probe_machine().filing;
let device = EmbedDevice::try_from(lane).unwrap_or(EmbedDevice::Cpu);
let mut embedder = Embedder::load("path/to/embeddinggemma-300m-q4", device)?;

let docs = embedder.embed_documents(&["found a secret room behind the waterfall"])?;
let query = embedder.embed_query("a hidden area")?;
let score: f32 = query.iter().zip(&docs[0]).map(|(a, b)| a * b).sum();
```

`embedder.device_in_use()` says where it runs, and `embedder.fall_off_reason()` says why, when the GPU refused and it fell back to the CPU. Errors are `EmbedError`: the model missing (with the file's path), the GPU unavailable, the model failing to load, or inference failing.

### The model

The library reads the model from a folder you give it and never downloads anything itself. `embed::catalog()` lists what that folder holds: EmbeddingGemma-300m in ONNX, q4 quantised (about 219 MB in all), from [onnx-community/embeddinggemma-300m-ONNX](https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX) on Hugging Face, pinned to one commit, with each file's size and sha256. Each entry is a `fetch::FetchSpec` from [pito-fetch](https://github.com/gmrdad82/pito-fetch), which downloads it resumably and checks the hash; any downloader works, as long as it checks the sha256.

EmbeddingGemma is Google's model, provided under and subject to the [Gemma Terms of Use](https://ai.google.dev/gemma/terms) and the [Gemma Prohibited Use Policy](https://ai.google.dev/gemma/prohibited_use_policy). pito-embed ships no weights; whoever downloads and uses the model accepts those terms, which the MIT licence here doesn't cover.

### The example

`examples/search.rs` is the program in the clip. It downloads the model the first time, into `$XDG_DATA_HOME/pito-embed/embeddinggemma-300m-q4` (`~/.local/share/…` when that isn't set) on Linux and `~/Library/Application Support/pito-embed/embeddinggemma-300m-q4` on macOS, or into the folder `PITO_EMBED_MODELS` names. Then it loads the model on the device the probe picks, embeds eight lines and ranks them against your query:

```sh
cargo run --release --example search -- a hidden area
```

## Build and test

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The tests that run the real model need it on disk: set `PITO_EMBED_MODELS` to the model's folder (running the example once fills the default one). Without it they say so and skip. CI runs all of it on Linux and macOS, with the model, on the CPU, since the runners have no GPU; the GPU paths run on the maintainer's machine.

## Contributing

Issues are welcome at [github.com/gmrdad82/pito-embed/issues](https://github.com/gmrdad82/pito-embed/issues): a machine where the GPU isn't picked or falls back when it shouldn't (with what `probe::probe_machine()` reports), a load or inference error, or results that look wrong. Pull requests aren't taken. Please read the [code of conduct](CODE_OF_CONDUCT.md) first, and report security issues privately, as [SECURITY.md](SECURITY.md) describes.

## Licence

The code is MIT licensed ([LICENSE](LICENSE)), © Catalin Ilinca. The model is Google's, under the Gemma Terms of Use, and the third-party notices are in [NOTICE](NOTICE). The PITO name and its logos are © Catalin Ilinca, all rights reserved, and are not covered by the MIT licence ([TRADEMARKS.md](TRADEMARKS.md)).
