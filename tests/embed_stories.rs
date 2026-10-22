use std::path::{Path, PathBuf};

use embed::{EmbedDevice, EmbedError, Embedder};

const REQUIRED: [&str; 6] = [
    "onnx/model_q4.onnx",
    "onnx/model_q4.onnx_data",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

fn scratch_dir(label: &str) -> tempfile::TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
    std::fs::create_dir_all(&root).expect("test scratch root is creatable");
    tempfile::Builder::new()
        .prefix(label)
        .tempdir_in(root)
        .expect("test scratch dir is creatable")
}

fn write_corrupt(dir: &Path, rel: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("every file has a parent"))
        .expect("model subdir is creatable");
    std::fs::write(&path, b"\x00\xffnot a model file\x7f").expect("corrupt file is writable");
}

fn corrupt_model_dir(label: &str, skip: Option<&str>) -> tempfile::TempDir {
    let dir = scratch_dir(label);
    for rel in REQUIRED.iter().filter(|rel| Some(**rel) != skip) {
        write_corrupt(dir.path(), rel);
    }
    dir
}

#[test]
fn a_dir_that_does_not_exist_reports_the_first_required_file() {
    let root = scratch_dir("absent");
    let missing = root.path().join("never-created");
    match Embedder::load(&missing, EmbedDevice::Cpu) {
        Err(EmbedError::ModelMissing { path }) => assert_eq!(path, missing.join(REQUIRED[0])),
        Err(other) => panic!("expected ModelMissing, got {other:?}"),
        Ok(_) => panic!("a missing dir must not load"),
    }
}

#[test]
fn each_required_file_missing_in_turn_is_named_while_the_other_five_are_present() {
    for (i, absent) in REQUIRED.iter().enumerate() {
        let dir = corrupt_model_dir(&format!("missing-{i}-"), Some(absent));
        match Embedder::load(dir.path(), EmbedDevice::Cpu) {
            Err(EmbedError::ModelMissing { path }) => {
                assert_eq!(path, dir.path().join(absent), "{absent}")
            }
            Err(other) => panic!("{absent} absent: expected ModelMissing, got {other:?}"),
            Ok(_) => panic!("{absent} absent: must not load"),
        }
    }
}

#[test]
fn a_missing_file_is_reported_on_the_compiled_device_too_before_any_provider_is_built() {
    let dir = corrupt_model_dir("missing-gpu-", Some("tokenizer.json"));
    match Embedder::load(dir.path(), EmbedDevice::compiled()) {
        Err(EmbedError::ModelMissing { path }) => {
            assert_eq!(path, dir.path().join("tokenizer.json"))
        }
        Err(other) => panic!("expected ModelMissing, got {other:?}"),
        Ok(_) => panic!("a dir without its tokenizer must not load"),
    }
}

#[test]
fn corrupt_files_on_the_processor_fail_to_load_with_a_bounded_detail() {
    let dir = corrupt_model_dir("corrupt-cpu-", None);
    match Embedder::load(dir.path(), EmbedDevice::Cpu) {
        Err(EmbedError::LoadFailed { detail }) => {
            assert!(!detail.is_empty());
            assert!(detail.len() <= 4096, "detail is {} bytes", detail.len());
        }
        Err(other) => panic!("expected LoadFailed, got {other:?}"),
        Ok(_) => panic!("corrupt files must not load"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_foreign_device_with_corrupt_files_fails_to_load_on_the_processor_too() {
    let dir = corrupt_model_dir("corrupt-foreign-", None);
    match Embedder::load(dir.path(), EmbedDevice::DirectMl) {
        Err(EmbedError::LoadFailed { detail }) => {
            assert!(!detail.is_empty());
            assert!(detail.len() <= 4096, "detail is {} bytes", detail.len());
        }
        Err(other) => panic!("expected LoadFailed, got {other:?}"),
        Ok(embedder) => panic!("corrupt files loaded on {:?}", embedder.device_in_use()),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_foreign_device_with_a_missing_file_reports_the_file_not_the_device() {
    let dir = corrupt_model_dir("missing-foreign-", Some("config.json"));
    match Embedder::load(dir.path(), EmbedDevice::DirectMl) {
        Err(EmbedError::ModelMissing { path }) => assert_eq!(path, dir.path().join("config.json")),
        Err(other) => panic!("expected ModelMissing, got {other:?}"),
        Ok(_) => panic!("a dir without its config must not load"),
    }
}

#[test]
fn every_error_prints_a_line_carrying_what_it_was_built_with() {
    let path = PathBuf::from("models/x/onnx/model_q4.onnx");
    let missing = EmbedError::ModelMissing { path: path.clone() }.to_string();
    assert!(missing.contains(&path.display().to_string()), "{missing}");

    for (err, detail) in [
        (
            EmbedError::GpuUnavailable {
                detail: "no adapter-7".into(),
            },
            "no adapter-7",
        ),
        (
            EmbedError::LoadFailed {
                detail: "bad graph-8".into(),
            },
            "bad graph-8",
        ),
        (
            EmbedError::Inference {
                detail: "shape mismatch-9".into(),
            },
            "shape mismatch-9",
        ),
    ] {
        let line = err.to_string();
        assert!(line.contains(detail), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }
}

#[test]
fn the_four_errors_print_distinct_lines_for_the_same_detail() {
    let lines = [
        EmbedError::ModelMissing {
            path: PathBuf::from("same"),
        },
        EmbedError::GpuUnavailable {
            detail: "same".into(),
        },
        EmbedError::LoadFailed {
            detail: "same".into(),
        },
        EmbedError::Inference {
            detail: "same".into(),
        },
    ]
    .map(|e| e.to_string());
    for (i, a) in lines.iter().enumerate() {
        for b in &lines[i + 1..] {
            assert_ne!(a, b);
        }
    }
}

#[cfg(unix)]
fn gate(args: &[&str], models: Option<&str>) -> std::process::Output {
    let mut cmd = std::process::Command::new("bash");
    cmd.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/gate"))
        .args(args)
        .env("NO_COLOR", "1")
        .env_remove("PITO_EMBED_MODELS");
    if let Some(m) = models {
        cmd.env("PITO_EMBED_MODELS", m);
    }
    cmd.output().expect("bash runs bin/gate")
}

#[cfg(unix)]
fn clap_shape(first: &str) -> String {
    format!("{first}\n\nUsage: bin/gate [--fast]\n\nFor more information, try '--help'.\n")
}

#[cfg(unix)]
#[test]
fn the_gate_reports_the_first_bad_argument_even_before_help() {
    let out = gate(&["--bogus", "--help"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        clap_shape("error: unexpected argument '--bogus' found")
    );
}

#[cfg(unix)]
#[test]
fn the_gate_refuses_a_repeated_fast_as_clap_does() {
    let out = gate(&["--fast", "--fast"], None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        clap_shape("error: the argument '--fast' cannot be used multiple times")
    );
}

#[cfg(unix)]
#[test]
fn the_gate_prints_help_on_stdout_when_help_comes_first() {
    let out = gate(&["--help", "--bogus"], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: bin/gate [--fast]"));
    assert!(out.stderr.is_empty());
}

#[cfg(unix)]
#[test]
fn the_full_gate_refuses_to_run_without_the_model_folder() {
    for models in [None, Some("")] {
        let out = gate(&[], models);
        assert_eq!(out.status.code(), Some(1), "{models:?}");
        assert!(out.stdout.is_empty(), "{models:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "bin/gate: the full gate needs PITO_EMBED_MODELS set to the model folder\n"
        );
    }
}
