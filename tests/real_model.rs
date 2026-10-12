use std::path::PathBuf;

use embed::{EmbedDevice, EmbedError, Embedder};

fn scratch_dir(label: &str) -> tempfile::TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
    std::fs::create_dir_all(&root).expect("test scratch root is creatable");
    tempfile::Builder::new()
        .prefix(label)
        .tempdir_in(root)
        .expect("test scratch dir is creatable")
}

fn model_dir() -> Option<PathBuf> {
    std::env::var_os("PITO_EMBED_MODELS").map(PathBuf::from)
}

fn resolve_device() -> Option<EmbedDevice> {
    EmbedDevice::try_from(probe::probe_machine().filing).ok()
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn assert_unit_norm(v: &[f32]) {
    let n = norm(v);
    assert!((n - 1.0).abs() < 1e-3, "expected unit norm, got {n}");
}

#[test]
fn embeddings_are_normalized_768_dims_and_prefixed() {
    let Some(dir) = model_dir() else {
        eprintln!("skip: PITO_EMBED_MODELS not set, real-model test not run");
        return;
    };
    let Some(device) = resolve_device() else {
        eprintln!("skip: no GPU lane on this machine, real-model test not run");
        return;
    };
    let mut embedder = match Embedder::load(&dir, device) {
        Ok(e) => e,
        Err(EmbedError::GpuUnavailable { detail }) => {
            eprintln!("skip: GPU unavailable, real-model test not run: {detail}");
            return;
        }
        Err(e) => panic!("unexpected error loading embedder: {e}"),
    };
    assert_eq!(embed::DIMS, 768);

    let query = embedder.embed_query("boss fight rage").unwrap();
    assert_eq!(query.len(), 768);
    assert_unit_norm(&query);

    let docs = embedder
        .embed_documents(&["oh come ON! I had him!", "calm run, patient, no dying"])
        .unwrap();
    assert_eq!(docs.len(), 2);
    for v in &docs {
        assert_eq!(v.len(), 768);
        assert_unit_norm(v);
    }

    let as_query = embedder.embed_query("hello world").unwrap();
    let as_doc = embedder.embed_documents(&["hello world"]).unwrap();
    assert_ne!(
        as_query, as_doc[0],
        "query and document prefixes must diverge for identical text"
    );

    let target = embedder
        .embed_documents(&["ninety-nine percent. it died at ninety-nine percent."])
        .unwrap();
    let related = embedder
        .embed_query("defeat snatched away at the very last second")
        .unwrap();
    let unrelated = embedder
        .embed_query("the weather today is sunny and calm")
        .unwrap();
    let sim_related = cosine(&related, &target[0]);
    let sim_unrelated = cosine(&unrelated, &target[0]);
    assert!(
        sim_related > sim_unrelated,
        "expected topical similarity to beat an unrelated line: {sim_related} vs {sim_unrelated}"
    );
}

#[test]
fn edge_cases_are_safe() {
    let Some(dir) = model_dir() else {
        eprintln!("skip: PITO_EMBED_MODELS not set, real-model test not run");
        return;
    };
    let Some(device) = resolve_device() else {
        eprintln!("skip: no GPU lane on this machine, real-model test not run");
        return;
    };
    let mut embedder = match Embedder::load(&dir, device) {
        Ok(e) => e,
        Err(EmbedError::GpuUnavailable { detail }) => {
            eprintln!("skip: GPU unavailable, real-model test not run: {detail}");
            return;
        }
        Err(e) => panic!("unexpected error loading embedder: {e}"),
    };

    assert!(embedder.embed_documents(&[]).unwrap().is_empty());

    let empty = embedder.embed_documents(&[""]).unwrap();
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[0].len(), 768);

    let long = "a".repeat(10_000);
    let huge = embedder.embed_documents(&[long.as_str()]).unwrap();
    assert_eq!(huge.len(), 1);
    assert_eq!(huge[0].len(), 768);
    assert_unit_norm(&huge[0]);

    let unicode = embedder
        .embed_documents(&["🙂🙂 țipete, café, 你好世界"])
        .unwrap();
    assert_eq!(unicode.len(), 1);
    assert_eq!(unicode[0].len(), 768);
    assert_unit_norm(&unicode[0]);

    let empty_query = embedder.embed_query("").unwrap();
    assert_eq!(empty_query.len(), 768);
}

#[test]
fn missing_model_dir_reports_model_missing_before_any_device_is_touched() {
    let dir = scratch_dir("missing-model");
    match Embedder::load(dir.path(), EmbedDevice::WebGpu) {
        Err(EmbedError::ModelMissing { path }) => {
            assert_eq!(path, dir.path().join("onnx").join("model_q4.onnx"));
        }
        Err(e) => panic!("expected ModelMissing, got {e}"),
        Ok(_) => panic!("expected ModelMissing, got Ok"),
    }
}

fn write_dummy_model_dir(dir: &std::path::Path, onnx_bytes: &[u8]) {
    let onnx_dir = dir.join("onnx");
    std::fs::create_dir_all(&onnx_dir).unwrap();
    std::fs::write(onnx_dir.join("model_q4.onnx"), onnx_bytes).unwrap();
    std::fs::write(
        onnx_dir.join("model_q4.onnx_data"),
        b"not real external weights",
    )
    .unwrap();
    std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
    std::fs::write(dir.join("config.json"), b"{}").unwrap();
    std::fs::write(dir.join("special_tokens_map.json"), b"{}").unwrap();
    std::fs::write(dir.join("tokenizer_config.json"), b"{}").unwrap();
}

#[test]
fn corrupt_model_files_report_load_failed_with_capped_detail() {
    let Some(device) = resolve_device() else {
        eprintln!("skip: no GPU lane on this machine, corrupt-model test not run");
        return;
    };
    let dir = scratch_dir("corrupt-model");
    write_dummy_model_dir(dir.path(), &[0u8; 64]);
    match Embedder::load(dir.path(), device) {
        Err(EmbedError::LoadFailed { detail }) => {
            assert!(!detail.is_empty());
            assert!(detail.len() <= 4096);
        }
        Err(EmbedError::GpuUnavailable { detail }) => {
            eprintln!("skip: GPU unavailable, corrupt-model test not run: {detail}");
        }
        Err(e) => panic!("expected LoadFailed, got {e}"),
        Ok(_) => panic!("expected LoadFailed, got Ok"),
    }
}

#[cfg(debug_assertions)]
#[test]
fn a_failing_card_provider_falls_to_the_processor() {
    let dir = scratch_dir("broken-card");
    write_dummy_model_dir(dir.path(), &[0u8; 64]);
    match Embedder::load(dir.path(), EmbedDevice::Broken) {
        Err(EmbedError::LoadFailed { .. }) => {}
        Err(e) => panic!("expected the processor's LoadFailed after the fall-off, got {e}"),
        Ok(_) => panic!("expected LoadFailed on a corrupt model, got Ok"),
    }

    let Some(dir) = model_dir() else {
        eprintln!("skip: PITO_EMBED_MODELS not set, the warm fall-off is not proven");
        return;
    };
    let mut embedder = Embedder::load(&dir, EmbedDevice::Broken)
        .expect("a broken card falls to the processor instead of failing");
    assert_eq!(embedder.device_in_use(), EmbedDevice::Cpu);
    let reason = embedder
        .fall_off_reason()
        .expect("the fall-off names the card's refusal");
    assert!(reason.contains("test door"), "reason: {reason}");
    assert!(reason.len() <= 4096);
    let query = embedder.embed_query("still searching by meaning").unwrap();
    assert_eq!(query.len(), 768);
    assert_unit_norm(&query);
}

#[test]
fn the_processor_asked_for_is_the_processor() {
    let dir = scratch_dir("cpu-missing-model");
    match Embedder::load(dir.path(), EmbedDevice::Cpu) {
        Err(EmbedError::ModelMissing { .. }) => {}
        Err(e) => panic!("expected ModelMissing, got {e}"),
        Ok(_) => panic!("expected ModelMissing, got Ok"),
    }

    let Some(dir) = model_dir() else {
        eprintln!("skip: PITO_EMBED_MODELS not set, the warm processor is not proven");
        return;
    };
    let embedder = Embedder::load(&dir, EmbedDevice::Cpu).expect("the processor always loads");
    assert_eq!(embedder.device_in_use(), EmbedDevice::Cpu);
    assert_eq!(embedder.fall_off_reason(), None);
}

#[test]
fn a_working_card_reports_no_fall_off() {
    let Some(dir) = model_dir() else {
        eprintln!("skip: PITO_EMBED_MODELS not set, real-model test not run");
        return;
    };
    let Some(device) = resolve_device() else {
        eprintln!("skip: no GPU lane on this machine, real-model test not run");
        return;
    };
    if device == EmbedDevice::Cpu {
        eprintln!("skip: the probe found no card, there is nothing to fall off");
        return;
    }
    let embedder = Embedder::load(&dir, device).expect("a card or the processor loads");
    if embedder.device_in_use() != device {
        eprintln!(
            "skip: the card fell to the processor here — {}",
            embedder.fall_off_reason().unwrap_or("no reason recorded")
        );
        return;
    }
    assert_eq!(embedder.device_in_use(), device);
    assert_eq!(embedder.fall_off_reason(), None);
}
