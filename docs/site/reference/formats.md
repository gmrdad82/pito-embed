---
title: Formats
description: What pito-embed promises about its vectors, the model folder it reads and the catalog of the model's files.
sidebar:
  order: 1
---

pito-embed writes no files and prints no JSON. These are the shapes it promises to code that uses it.

## Vectors

- `embed_query` returns a `Vec<f32>`; `embed_documents` returns a `Vec<Vec<f32>>`, one vector per input line, in the input's order.
- Every vector has `embed::DIMS` numbers: 768.
- Every vector has unit length (L2 norm 1), so the dot product of two vectors is their cosine similarity, from -1 to 1.
- Queries carry EmbeddingGemma's query prompt (`task: search result | query: `) and documents its document prompt (`title: none | text: `), added by the library; compare queries with documents.
- Each input is cut at 128 tokens.

## The model folder

`Embedder::load(dir, device)` reads six files from `dir`, laid out as on Hugging Face:

```text
<dir>/
├── config.json
├── special_tokens_map.json
├── tokenizer.json
├── tokenizer_config.json
└── onnx/
    ├── model_q4.onnx
    └── model_q4.onnx_data
```

A missing file is `EmbedError::ModelMissing` with that file's path.

## The catalog

`embed::catalog()` returns `&'static [fetch::FetchSpec]`, one entry per file above, all from [onnx-community/embeddinggemma-300m-ONNX](https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX) at commit `5090578d9565bb06545b4552f76e6bc2c93e4a66`:

| Field | Type | Holds |
| --- | --- | --- |
| `url` | `&'static str` | the file's download address, pinned to the commit |
| `dest_rel` | `&'static str` | `models/embeddinggemma-300m-q4/` followed by the file's path in the folder above |
| `sha256` | `&'static str` | the file's sha256, 64 lowercase hex characters |
| `size_hint` | `Option<u64>` | the file's size in bytes, always present |

| File | Bytes | sha256 |
| --- | ---: | --- |
| `config.json` | 1,765 | `6e1f06404b7163e0325ed2ea3e6781cde50f4a50b31780a95ad0d30e8404d77b` |
| `special_tokens_map.json` | 662 | `2f7b0adf4fb469770bb1490e3e35df87b1dc578246c5e7e6fc76ecf33213a397` |
| `tokenizer_config.json` | 1,156,830 | `3ca953eea6c3c9fcda9cf3df22949ff18b216f7c74bd6459230f3f1013953f3a` |
| `tokenizer.json` | 20,323,312 | `4dda02faaf32bc91031dc8c88457ac272b00c1016cc679757d1c441b248b9c47` |
| `onnx/model_q4.onnx` | 519,322 | `ad1dfee81a70f7944b9b9d1cc6e48075b832881cf33fab2f2b248be78f3f0043` |
| `onnx/model_q4.onnx_data` | 196,725,760 | `599962c3143b040de2dd05e5975be3e9091dd067cacc6a8f7186e3203bab9e02` |

## The examples' model folder

The search example keeps the model in `$PITO_EMBED_MODELS` when it is set, otherwise in `$XDG_DATA_HOME/pito-embed/embeddinggemma-300m-q4` (`~/.local/share/pito-embed/embeddinggemma-300m-q4` when that isn't set) on Linux and `~/Library/Application Support/pito-embed/embeddinggemma-300m-q4` on macOS. The bench example and the tests that run the real model read `PITO_EMBED_MODELS`.
