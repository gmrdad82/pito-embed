---
title: Get the model
description: Download EmbeddingGemma-300m's pinned files, check their sha256, and point the library at the folder.
sidebar:
  order: 2
---

The library never downloads anything. It reads the model from a folder you give `Embedder::load`, and `embed::catalog()` tells you what that folder must hold: the six files of EmbeddingGemma-300m in ONNX, q4 quantised (about 219 MB), from [onnx-community/embeddinggemma-300m-ONNX](https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX) on Hugging Face, each pinned to commit `5090578d9565bb06545b4552f76e6bc2c93e4a66` with its sha256 and size.

## The easy way: the search example

The search example downloads the model the first time it runs, checks every file's sha256 and keeps it for next time:

```sh
cargo run --release --example search -- a hidden area
```

It keeps the model in `$XDG_DATA_HOME/pito-embed/embeddinggemma-300m-q4` (`~/.local/share/…` when that isn't set) on Linux and `~/Library/Application Support/pito-embed/embeddinggemma-300m-q4` on macOS. Set `PITO_EMBED_MODELS` to use another folder:

```sh
PITO_EMBED_MODELS=models/embeddinggemma-300m-q4 cargo run --release --example search -- a hidden area
```

Pass that folder to `Embedder::load`.

## In your own program

Each catalog entry is a `fetch::FetchSpec` from [pito-fetch](https://docs.pitomd.com/pito-fetch/): the file's `url`, its `sha256`, its `size_hint` in bytes and its `dest_rel`, a path that starts with `models/embeddinggemma-300m-q4/`. Strip that prefix to get where the file goes inside the model folder. `examples/search.rs` does it with pito-fetch, which resumes a broken download and checks the hash. Add it next to `embed`:

```toml
fetch = { git = "https://github.com/gmrdad82/pito-fetch.git", tag = "v1.0.1" }
```

Then:

```rust
use fetch::{CancelToken, Fetcher, Request};

let fetcher = Fetcher::new(&dir);
let cancel = CancelToken::new();
for spec in embed::catalog() {
    let rel = spec.dest_rel.trim_start_matches("models/embeddinggemma-300m-q4/");
    let request = Request {
        url: spec.url.to_string(),
        dest_rel: rel.to_string(),
        sha256: spec.sha256.to_string(),
        size_hint: spec.size_hint,
    };
    fetcher.fetch_request(&request, &cancel, |_progress| {})?;
}
```

Any downloader works, as long as it checks the sha256. The folder's layout is in [Formats](../reference/formats.md).

## The model's terms

EmbeddingGemma is Google's model, provided under and subject to the [Gemma Terms of Use](https://ai.google.dev/gemma/terms) and the [Gemma Prohibited Use Policy](https://ai.google.dev/gemma/prohibited_use_policy). pito-embed ships no weights; whoever downloads and uses the model accepts those terms, which the MIT licence of the code doesn't cover.
