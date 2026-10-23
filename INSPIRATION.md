# Inspiration

Where code in this repository was ported from, with the version it was taken from.

- `tools/ort-cache/src/ustar.rs`: the ustar reader is ported from ort-sys 2.0.0-rc.13, `build/download/extract.rs` (https://github.com/pykeio/ort, MIT OR Apache-2.0, used here under MIT; its copyright and licence text are in NOTICE). It keeps that file's structure (512-byte headers, the octal size and checksum fields, the ustar prefix, the padding to the next block) and is stricter: it accepts only regular files at the top level, one leading `./` allowed, refuses everything else, and never overwrites a file.
- `tools/ort-cache/src/dist.rs` and `tools/ort-cache/src/dirs.rs` follow ort-sys 2.0.0-rc.13's `build/download/mod.rs` and `src/internal/dirs.rs`: the same `dist.tsv` table, exact feature-set match, and cache folder, so the tool fills the folder ort-sys reads.
