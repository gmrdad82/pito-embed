# Changelog

## v1.0.4 — 2026-10-23

### Added

- `bin/gate` runs the checks a change must pass: `--fast` for formatting, lints, the tests without the model and the doc tests; without it the full gate, locked, with the search example and the tests on the model.
- `bin/ort-cache` fetches the pinned, hash-checked ONNX Runtime build into the cache `ort` reads, and `bin/gate` runs it first when `CARGO_NET_OFFLINE` is set, so the gate builds and links on a machine where cargo stays offline.

### Changed

- GitHub Actions no longer runs on the repo; the maintainer builds, tests and publishes each release.
- The README links the PITO MD Discord for questions.

### Fixed

- `probe` no longer picks Quick Sync as the video lane on a machine without an Intel card.
- An `nvidia-smi` that can't run (not executable, or a folder) no longer makes `probe` count the machine as NVIDIA.
- A huge VRAM reading from `nvidia-smi` no longer overflows.
- Embeddings stay at unit length when their values are tiny or huge.
- The spec line no longer starts with a stray `·` when the card's name is empty, and just under 1 GB it reads "1 GB" instead of "1024 MB".
- A model file that is a folder or can't be read now fails to load with its path and the reason, instead of being reported as missing; a missing file is reported before the large weights file is read.

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
