# Changelog

## v1.0.3 — 2026-10-16

### Changed

- The model downloader pito-fetch moves to v1.0.2.
- The README's install lines point at v1.0.3.
- The README and NOTICE credit pcod (pito-codecs) for the clips.

## v1.0.2 — 2026-10-16

### Changed

- The model download example and guide use pito-fetch v1.0.1.
- The README's install lines pin v1.0.2, and its Licence section shows the PITO MD wordmark.
- CI saves its caches from main only.

## v1.0.1 — 2026-10-14

### Added

- A docs section for docs.pitomd.com: an overview, install, guides to searching by meaning, getting the model, picking a device, knowing the machine, measuring the speed and shipping a binary, the formats, the API reference and this changelog. Every Release now carries it as `docs-vX.Y.Z.tar.gz`, built by `bin/docs-bundle`.

### Changed

- `embed` and `probe` are both 1.0.1; pin them at `tag = "v1.0.1"`. The library is unchanged.

## v1.0.0 — 2026-10-14

### Added

- `examples/bench.rs` times loading the model, a query and 256 lines on your GPU and on your CPU: on an RX 9060 XT over Vulkan, 256 lines take 477 ms, 6.3× faster than the CPU.
- `examples/machine.rs` prints what `probe` finds: the system, the card, the best lane for each kind of work, the adapters and the free disk space.

### Changed

- `embed` and `probe` are both 1.0.0; pin them at `tag = "v1.0.0"`.
- A new README with clips of the search, the bench and the machine probe, and a speed chart.
- The brand now reads PITO MD.

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
