# Changelog

## v0.3.0 — 2026-10-12

### Added

- `examples/search.rs`: downloads the model into the standard data folder, loads it on the device the probe picks and ranks eight lines against a query.

### Changed

- The model catalog points at the upstream files on Hugging Face, onnx-community/embeddinggemma-300m-ONNX pinned to one commit, with the same sha256 as before.
- Both crates are MIT licensed, and pito-fetch v0.1.2 is pinned over https.

## v0.2.0 — 2026-10-07

### Added

- The `probe` crate joins the workspace in `crates/probe`, so `embed` and `probe` are pinned from one repository at one tag.

## v0.1.1 — 2026-09-28

### Changed

- pito-fetch v0.1.1, which polls body reads on ureq 3.4.1 and later.

## v0.1.0 — 2026-09-28

### Added

- On-device EmbeddingGemma-300m through fastembed-rs, on the GPU or the CPU, with a catalog of the model's files and their sha256.
