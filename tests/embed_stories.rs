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
fn a_foreign_device_with_corrupt_files_ends_in_a_bounded_load_failure() {
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
fn a_required_path_that_is_a_directory_fails_to_load_naming_it() {
    for (i, rel) in REQUIRED.iter().enumerate() {
        let dir = corrupt_model_dir(&format!("dir-{i}-"), Some(rel));
        std::fs::create_dir_all(dir.path().join(rel)).expect("a folder in the file's place");
        for device in [EmbedDevice::Cpu, EmbedDevice::compiled()] {
            match Embedder::load(dir.path(), device) {
                Err(EmbedError::LoadFailed { detail }) => assert!(
                    detail.contains(&dir.path().join(rel).display().to_string()),
                    "{rel} on {device:?}: {detail}"
                ),
                Err(other) => panic!("{rel} on {device:?}: expected LoadFailed, got {other:?}"),
                Ok(_) => panic!("{rel} on {device:?}: a folder must not load as a file"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn a_missing_file_is_named_before_any_earlier_file_is_read() {
    use std::os::unix::fs::PermissionsExt;

    let dir = corrupt_model_dir("unread-", Some("tokenizer_config.json"));
    let data = dir.path().join(REQUIRED[1]);
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o000))
        .expect("the weights file can be locked");
    let outcome = Embedder::load(dir.path(), EmbedDevice::Cpu);
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o644))
        .expect("the weights file can be unlocked");
    match outcome {
        Err(EmbedError::ModelMissing { path }) => {
            assert_eq!(path, dir.path().join("tokenizer_config.json"))
        }
        Err(other) => panic!("expected ModelMissing, got {other:?}"),
        Ok(_) => panic!("a dir without its tokenizer config must not load"),
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_required_file_fails_to_load_naming_it_not_as_missing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = corrupt_model_dir("unreadable-", None);
    let config = dir.path().join("config.json");
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000))
        .expect("the config can be locked");
    if std::fs::read(&config).is_ok() {
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
        eprintln!("skipped: this user reads files whatever their mode");
        return;
    }
    let outcome = Embedder::load(dir.path(), EmbedDevice::Cpu);
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644))
        .expect("the config can be unlocked");
    match outcome {
        Err(EmbedError::LoadFailed { detail }) => {
            assert!(detail.contains(&config.display().to_string()), "{detail}");
            assert!(detail.len() <= 4096, "detail is {} bytes", detail.len());
        }
        Err(other) => panic!("expected LoadFailed, got {other:?}"),
        Ok(_) => panic!("an unreadable config must not load"),
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
