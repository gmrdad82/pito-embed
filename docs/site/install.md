---
title: Install
description: Add embed and probe to a Rust project on Linux, macOS or WSL, and run the first search.
sidebar:
  order: 2
---

pito-embed is a Rust library. It isn't on crates.io: both crates come from the GitHub repository and pin the same tag. It needs Rust 1.97 or newer, and the build downloads ONNX Runtime's prebuilt libraries through the [`ort`](https://github.com/pykeio/ort) crate.

## Linux

Debian and Ubuntu; other distributions need the same three packages.

```sh
sudo apt-get install -y build-essential pkg-config libssl-dev
```

## macOS

Xcode's command line tools:

```sh
xcode-select --install
```

## Windows

Use WSL and follow the Linux steps inside it.

## Add it to your project

```toml
[dependencies]
embed = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v@VERSION@" }
probe = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v@VERSION@" }
```

The default `vulkan` feature runs the model on the GPU on Linux. For a CPU-only build, turn it off:

```toml
embed = { git = "https://github.com/gmrdad82/pito-embed.git", tag = "v@VERSION@", default-features = false }
```

## Quick start

Clone the repository and run the search example. It downloads the model once (219 MB, sha256 checked), loads it on your GPU and ranks eight lines against your query:

```sh
git clone https://github.com/gmrdad82/pito-embed && cd pito-embed
cargo run --release --example search -- a hidden area
```

```text
downloaded config.json, sha256 checked
…
downloaded onnx/model_q4.onnx_data, sha256 checked
loaded EmbeddingGemma-300m on Vulkan in 1725 ms
embedded 8 lines and the query in 93 ms, 768 dimensions each

query: a hidden area
  0.567  found a secret room behind the waterfall
  0.295  we finally beat the final boss on hard
  0.289  the soundtrack in this level is incredible
```

The download lines show only on the first run. On a machine without a GPU the first line names the processor instead of Vulkan. Then read [Search by meaning](guides/search-by-meaning.md) to do the same in your own code.
