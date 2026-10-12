use fetch::FetchSpec;

const GEMMA: [FetchSpec; 6] = [
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/config.json",
        dest_rel: "models/embeddinggemma-300m-q4/config.json",
        sha256: "6e1f06404b7163e0325ed2ea3e6781cde50f4a50b31780a95ad0d30e8404d77b",
        size_hint: Some(1_765),
    },
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/special_tokens_map.json",
        dest_rel: "models/embeddinggemma-300m-q4/special_tokens_map.json",
        sha256: "2f7b0adf4fb469770bb1490e3e35df87b1dc578246c5e7e6fc76ecf33213a397",
        size_hint: Some(662),
    },
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/tokenizer_config.json",
        dest_rel: "models/embeddinggemma-300m-q4/tokenizer_config.json",
        sha256: "3ca953eea6c3c9fcda9cf3df22949ff18b216f7c74bd6459230f3f1013953f3a",
        size_hint: Some(1_156_830),
    },
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/tokenizer.json",
        dest_rel: "models/embeddinggemma-300m-q4/tokenizer.json",
        sha256: "4dda02faaf32bc91031dc8c88457ac272b00c1016cc679757d1c441b248b9c47",
        size_hint: Some(20_323_312),
    },
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/onnx/model_q4.onnx",
        dest_rel: "models/embeddinggemma-300m-q4/onnx/model_q4.onnx",
        sha256: "ad1dfee81a70f7944b9b9d1cc6e48075b832881cf33fab2f2b248be78f3f0043",
        size_hint: Some(519_322),
    },
    FetchSpec {
        url: "https://huggingface.co/onnx-community/embeddinggemma-300m-ONNX/resolve/5090578d9565bb06545b4552f76e6bc2c93e4a66/onnx/model_q4.onnx_data",
        dest_rel: "models/embeddinggemma-300m-q4/onnx/model_q4.onnx_data",
        sha256: "599962c3143b040de2dd05e5975be3e9091dd067cacc6a8f7186e3203bab9e02",
        size_hint: Some(196_725_760),
    },
];

pub fn catalog() -> &'static [FetchSpec] {
    &GEMMA
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_all_six_gemma_files() {
        assert_eq!(catalog().len(), 6);
    }

    #[test]
    fn every_entry_lands_under_the_pinned_model_dir() {
        for spec in catalog() {
            assert!(spec.dest_rel.starts_with("models/embeddinggemma-300m-q4/"));
        }
    }

    #[test]
    fn every_entry_carries_a_sha256_and_a_size_hint() {
        for spec in catalog() {
            assert_eq!(spec.sha256.len(), 64);
            assert!(spec.size_hint.is_some());
        }
    }
}
