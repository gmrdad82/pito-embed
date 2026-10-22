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

#[cfg(unix)]
struct Gate {
    scratch: tempfile::TempDir,
}

#[cfg(unix)]
struct GateRun {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    cargo_calls: Vec<String>,
}

#[cfg(unix)]
impl Gate {
    fn new(label: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let scratch = scratch_dir(label);
        let stub = scratch.path().join("bin/cargo");
        std::fs::create_dir_all(stub.parent().unwrap()).expect("stub folder is creatable");
        std::fs::write(
            &stub,
            "#!/bin/sh\n\
             printf '%s|%s\\n' \"${PITO_EMBED_MODELS-unset}\" \"$*\" >> \"$STUB_CARGO_LOG\"\n\
             if [ \"$1\" = \"$STUB_CARGO_FAIL\" ]; then exit 7; fi\n\
             exit 0\n",
        )
        .expect("stub cargo is writable");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("stub cargo is executable");
        Self { scratch }
    }

    fn run(&self, args: &[&str], models: Option<&str>, fail: Option<&str>) -> GateRun {
        let log = self.scratch.path().join("cargo.log");
        let _ = std::fs::remove_file(&log);
        let path = std::env::join_paths(std::iter::once(self.scratch.path().join("bin")).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .expect("PATH joins");
        let mut cmd = std::process::Command::new("bash");
        cmd.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/gate"))
            .args(args)
            .env("PATH", path)
            .env("NO_COLOR", "1")
            .env("STUB_CARGO_LOG", &log)
            .env("STUB_CARGO_FAIL", fail.unwrap_or("none"))
            .env_remove("PITO_EMBED_MODELS");
        if let Some(m) = models {
            cmd.env("PITO_EMBED_MODELS", m);
        }
        let out = cmd.output().expect("bash runs bin/gate");
        GateRun {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            cargo_calls: std::fs::read_to_string(&log)
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

#[cfg(unix)]
const FAST_STEPS: [&str; 4] = [
    "models|fmt --all --check",
    "models|clippy --workspace --all-targets -- -D warnings",
    "unset|nextest run --workspace --no-tests=pass",
    "models|test --workspace --doc",
];

#[cfg(unix)]
const FULL_STEPS: [&str; 5] = [
    "models|fmt --all --check",
    "models|clippy --workspace --all-targets --locked -- -D warnings",
    "models|test --workspace --doc --locked",
    "models|run --release --locked --example search -- a hidden area",
    "models|nextest run --workspace --locked --no-tests=pass",
];

#[cfg(unix)]
fn clap_shape(first: &str) -> String {
    format!("{first}\n\nUsage: bin/gate [--fast]\n\nFor more information, try '--help'.\n")
}

#[cfg(unix)]
#[test]
fn the_gate_reports_the_first_bad_argument_even_before_help() {
    let run = Gate::new("gate-bogus-").run(&["--bogus", "--help"], None, None);
    assert_eq!(run.status, Some(2));
    assert!(run.stdout.is_empty());
    assert_eq!(
        run.stderr,
        clap_shape("error: unexpected argument '--bogus' found")
    );
    assert!(run.cargo_calls.is_empty(), "{:?}", run.cargo_calls);
}

#[cfg(unix)]
#[test]
fn the_gate_refuses_a_repeated_fast_as_clap_does() {
    let run = Gate::new("gate-repeat-").run(&["--fast", "--fast"], None, None);
    assert_eq!(run.status, Some(2));
    assert_eq!(
        run.stderr,
        clap_shape("error: the argument '--fast' cannot be used multiple times")
    );
    assert!(run.cargo_calls.is_empty(), "{:?}", run.cargo_calls);
}

#[cfg(unix)]
#[test]
fn the_gate_prints_help_on_stdout_for_either_help_flag() {
    let gate = Gate::new("gate-help-");
    for args in [&["--help", "--bogus"][..], &["-h"], &["--fast", "-h"]] {
        let run = gate.run(args, None, None);
        assert_eq!(run.status, Some(0), "{args:?}");
        assert!(run.stdout.contains("Usage: bin/gate [--fast]"), "{args:?}");
        assert!(run.stderr.is_empty(), "{args:?}");
        assert!(
            run.cargo_calls.is_empty(),
            "{args:?}: {:?}",
            run.cargo_calls
        );
    }
}

#[cfg(unix)]
#[test]
fn the_full_gate_refuses_to_run_without_the_model_folder() {
    let gate = Gate::new("gate-nomodel-");
    for models in [None, Some("")] {
        let run = gate.run(&[], models, None);
        assert_eq!(run.status, Some(1), "{models:?}");
        assert!(run.stdout.is_empty(), "{models:?}");
        assert_eq!(
            run.stderr,
            "bin/gate: the full gate needs PITO_EMBED_MODELS set to the model folder\n"
        );
        assert!(
            run.cargo_calls.is_empty(),
            "{models:?}: {:?}",
            run.cargo_calls
        );
    }
}

#[cfg(unix)]
#[test]
fn the_fast_gate_runs_its_four_steps_in_order_without_the_model_and_ends_green() {
    let gate = Gate::new("gate-fast-");
    for models in [None, Some("models")] {
        let run = gate.run(&["--fast"], models, None);
        assert_eq!(run.status, Some(0), "{models:?}: {}", run.stderr);
        let expected: Vec<String> = FAST_STEPS
            .iter()
            .map(|step| match models {
                Some(_) => step.to_string(),
                None => step.replace("models|", "unset|"),
            })
            .collect();
        assert_eq!(run.cargo_calls, expected, "{models:?}");
        assert_eq!(run.stdout.lines().last(), Some("fast gate: green"));
    }
}

#[cfg(unix)]
#[test]
fn a_failing_fast_step_stops_the_gate_with_its_status_and_no_green_line() {
    let gate = Gate::new("gate-fast-fail-");
    for (i, step) in ["fmt", "clippy", "nextest", "test"].iter().enumerate() {
        let run = gate.run(&["--fast"], Some("models"), Some(step));
        assert_eq!(run.status, Some(7), "{step}");
        assert_eq!(run.cargo_calls, FAST_STEPS[..=i], "{step}");
        assert!(!run.stdout.contains("green"), "{step}: {}", run.stdout);
    }
}

#[cfg(unix)]
#[test]
fn the_full_gate_locks_every_build_and_runs_the_example_before_the_model_tests() {
    let run = Gate::new("gate-full-").run(&[], Some("models"), None);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(run.cargo_calls, FULL_STEPS);
    assert_eq!(run.stdout.lines().last(), Some("gate: green"));
}

#[cfg(unix)]
#[test]
fn a_failing_full_step_stops_the_gate_with_its_status_and_no_green_line() {
    let gate = Gate::new("gate-full-fail-");
    for (i, step) in ["fmt", "clippy", "test", "run", "nextest"]
        .iter()
        .enumerate()
    {
        let run = gate.run(&[], Some("models"), Some(step));
        assert_eq!(run.status, Some(7), "{step}");
        assert_eq!(run.cargo_calls, FULL_STEPS[..=i], "{step}");
        assert!(!run.stdout.contains("green"), "{step}: {}", run.stdout);
    }
}
