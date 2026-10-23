#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch_dir(label: &str) -> tempfile::TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
    std::fs::create_dir_all(&root).expect("test scratch root is creatable");
    tempfile::Builder::new()
        .prefix(label)
        .tempdir_in(root)
        .expect("test scratch dir is creatable")
}

fn write_exe(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).expect("stub folder is creatable");
    std::fs::write(path, body).expect("stub is writable");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("stub is executable");
}

fn real_path() -> std::ffi::OsString {
    std::env::var_os("PATH").unwrap_or_default()
}

fn find_tool(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&real_path())
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

struct Bench {
    scratch: tempfile::TempDir,
}

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    cargo: Vec<String>,
}

impl Bench {
    fn new(label: &str) -> Self {
        let scratch = scratch_dir(label);
        write_exe(
            &scratch.path().join("bin/cargo"),
            "#!/bin/sh\n\
             printf '%s|%s|%s\\n' \"${CARGO_NET_OFFLINE-unset}\" \"${PITO_EMBED_MODELS-unset}\" \"$*\" >> \"$STUB_LOG\"\n\
             if [ \"$1\" = \"${STUB_CARGO_FAIL-none}\" ]; then exit 7; fi\n\
             exit 0\n",
        );
        Self { scratch }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.scratch.path().join(rel)
    }

    fn run(&self, script: &str, args: &[&str], env: &[(&str, &str)]) -> Run {
        let log = self.path("cargo.log");
        let _ = std::fs::remove_file(&log);
        let path = std::env::join_paths(
            std::iter::once(self.path("bin")).chain(std::env::split_paths(&real_path())),
        )
        .expect("PATH joins");
        let mut cmd = Command::new(find_tool("bash").expect("bash is on PATH"));
        cmd.arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("bin")
                .join(script),
        )
        .args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", self.path("home"))
        .env("NO_COLOR", "1")
        .env("STUB_LOG", &log);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("bash runs the script");
        Run {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            cargo: std::fs::read_to_string(&log)
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

const FAST_STEPS: [&str; 4] = [
    "models|fmt --all --check",
    "models|clippy --workspace --all-targets -- -D warnings",
    "unset|nextest run --workspace --no-tests=pass",
    "models|test --workspace --doc",
];

const FULL_STEPS: [&str; 5] = [
    "models|fmt --all --check",
    "models|clippy --workspace --all-targets --locked -- -D warnings",
    "models|test --workspace --doc --locked",
    "models|run --release --locked --example search -- a hidden area",
    "models|nextest run --workspace --locked --no-tests=pass",
];

fn online(steps: &[&str], net: &str) -> Vec<String> {
    steps.iter().map(|s| format!("{net}|{s}")).collect()
}

fn seeding(net: &str, models: &str) -> String {
    format!("{net}|{models}|run -q --locked --offline -p ort-cache --")
}

fn fast_offline(net: &str) -> Vec<String> {
    vec![
        seeding(net, "models"),
        format!("{net}|models|fmt --all --check"),
        "unset|models|clippy --offline --workspace --all-targets -- -D warnings".into(),
        "unset|unset|nextest run --offline --workspace --no-tests=pass".into(),
        "unset|models|test --offline --workspace --doc".into(),
    ]
}

fn full_offline(net: &str) -> Vec<String> {
    vec![
        seeding(net, "models"),
        format!("{net}|models|fmt --all --check"),
        "unset|models|clippy --offline --workspace --all-targets --locked -- -D warnings".into(),
        "unset|models|test --offline --workspace --doc --locked".into(),
        "unset|models|run --offline --release --locked --example search -- a hidden area".into(),
        "unset|models|nextest run --offline --workspace --locked --no-tests=pass".into(),
    ]
}

fn clap_shape(bin: &str, usage: &str, first: &str) -> String {
    format!("{first}\n\nUsage: {bin} {usage}\n\nFor more information, try '--help'.\n")
}

#[test]
fn the_gate_reports_the_first_bad_argument_even_before_help() {
    let run = Bench::new("gate-bogus-").run("gate", &["--bogus", "--help"], &[]);
    assert_eq!(run.status, Some(2));
    assert!(run.stdout.is_empty());
    assert_eq!(
        run.stderr,
        clap_shape(
            "bin/gate",
            "[--fast]",
            "error: unexpected argument '--bogus' found"
        )
    );
    assert!(run.cargo.is_empty(), "{:?}", run.cargo);
}

#[test]
fn the_gate_refuses_a_repeated_fast_as_clap_does() {
    let run = Bench::new("gate-repeat-").run("gate", &["--fast", "--fast"], &[]);
    assert_eq!(run.status, Some(2));
    assert_eq!(
        run.stderr,
        clap_shape(
            "bin/gate",
            "[--fast]",
            "error: the argument '--fast' cannot be used multiple times"
        )
    );
    assert!(run.cargo.is_empty(), "{:?}", run.cargo);
}

#[test]
fn the_gate_prints_help_on_stdout_for_either_help_flag() {
    let bench = Bench::new("gate-help-");
    for args in [&["--help", "--bogus"][..], &["-h"], &["--fast", "-h"]] {
        let run = bench.run("gate", args, &[("CARGO_NET_OFFLINE", "true")]);
        assert_eq!(run.status, Some(0), "{args:?}");
        assert!(run.stdout.contains("Usage: bin/gate [--fast]"), "{args:?}");
        assert!(run.stdout.contains("CARGO_NET_OFFLINE"), "{args:?}");
        assert!(run.stderr.is_empty(), "{args:?}");
        assert!(run.cargo.is_empty(), "{args:?}: {:?}", run.cargo);
    }
}

#[test]
fn the_full_gate_refuses_to_run_without_the_model_folder_online_or_offline() {
    let bench = Bench::new("gate-nomodel-");
    for net in [None, Some("true")] {
        for models in [None, Some("")] {
            let mut env = vec![];
            env.extend(models.map(|m| ("PITO_EMBED_MODELS", m)));
            env.extend(net.map(|n| ("CARGO_NET_OFFLINE", n)));
            let run = bench.run("gate", &[], &env);
            assert_eq!(run.status, Some(1), "{env:?}");
            assert!(run.stdout.is_empty(), "{env:?}");
            assert_eq!(
                run.stderr,
                "bin/gate: the full gate needs PITO_EMBED_MODELS set to the model folder\n"
            );
            assert!(run.cargo.is_empty(), "{env:?}: {:?}", run.cargo);
        }
    }
}

#[test]
fn the_fast_gate_runs_its_four_steps_in_order_without_the_model_and_ends_green() {
    let bench = Bench::new("gate-fast-");
    for models in [None, Some("models")] {
        let env: Vec<_> = models
            .map(|m| ("PITO_EMBED_MODELS", m))
            .into_iter()
            .collect();
        let run = bench.run("gate", &["--fast"], &env);
        assert_eq!(run.status, Some(0), "{models:?}: {}", run.stderr);
        let expected: Vec<String> = online(&FAST_STEPS, "unset")
            .into_iter()
            .map(|step| match models {
                Some(_) => step,
                None => step.replace("|models|", "|unset|"),
            })
            .collect();
        assert_eq!(run.cargo, expected, "{models:?}");
        assert_eq!(run.stdout.lines().last(), Some("fast gate: green"));
    }
}

#[test]
fn a_failing_fast_step_stops_the_gate_with_its_status_and_no_green_line() {
    let bench = Bench::new("gate-fast-fail-");
    for (i, step) in ["fmt", "clippy", "nextest", "test"].iter().enumerate() {
        let run = bench.run(
            "gate",
            &["--fast"],
            &[("PITO_EMBED_MODELS", "models"), ("STUB_CARGO_FAIL", step)],
        );
        assert_eq!(run.status, Some(7), "{step}");
        assert_eq!(run.cargo, online(&FAST_STEPS[..=i], "unset"), "{step}");
        assert!(!run.stdout.contains("green"), "{step}: {}", run.stdout);
    }
}

#[test]
fn the_full_gate_locks_every_build_and_runs_the_example_before_the_model_tests() {
    let run = Bench::new("gate-full-").run("gate", &[], &[("PITO_EMBED_MODELS", "models")]);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(run.cargo, online(&FULL_STEPS, "unset"));
    assert_eq!(run.stdout.lines().last(), Some("gate: green"));
}

#[test]
fn a_failing_full_step_stops_the_gate_with_its_status_and_no_green_line() {
    let bench = Bench::new("gate-full-fail-");
    for (i, step) in ["fmt", "clippy", "test", "run", "nextest"]
        .iter()
        .enumerate()
    {
        let run = bench.run(
            "gate",
            &[],
            &[("PITO_EMBED_MODELS", "models"), ("STUB_CARGO_FAIL", step)],
        );
        assert_eq!(run.status, Some(7), "{step}");
        assert_eq!(run.cargo, online(&FULL_STEPS[..=i], "unset"), "{step}");
        assert!(!run.stdout.contains("green"), "{step}: {}", run.stdout);
    }
}
#[test]
fn a_cargo_net_offline_that_is_not_one_or_true_leaves_the_gate_as_it_was() {
    let bench = Bench::new("gate-falsy-");
    for net in ["0", "false", "yes", ""] {
        let env = [("PITO_EMBED_MODELS", "models"), ("CARGO_NET_OFFLINE", net)];
        let run = bench.run("gate", &["--fast"], &env);
        assert_eq!(run.status, Some(0), "{net:?}: {}", run.stderr);
        assert_eq!(run.cargo, online(&FAST_STEPS, net), "{net:?}");
        let run = bench.run("gate", &[], &env);
        assert_eq!(run.cargo, online(&FULL_STEPS, net), "{net:?}");
    }
}

#[test]
fn the_offline_gate_seeds_ort_then_runs_every_cargo_step_offline_without_the_variable() {
    let bench = Bench::new("gate-offline-");
    for net in ["1", "true", "TRUE", "True"] {
        let env = [("PITO_EMBED_MODELS", "models"), ("CARGO_NET_OFFLINE", net)];
        for (args, steps, green) in [
            (&["--fast"][..], fast_offline(net), "fast gate: green"),
            (&[], full_offline(net), "gate: green"),
        ] {
            let run = bench.run("gate", args, &env);
            assert_eq!(run.status, Some(0), "{net} {args:?}: {}", run.stderr);
            assert_eq!(run.cargo, steps, "{net} {args:?}");
            assert_eq!(run.stdout.lines().last(), Some(green));
        }
    }
}

#[test]
fn the_offline_gate_stops_before_any_step_when_ort_cache_fails() {
    let bench = Bench::new("gate-offline-refused-");
    for args in [&["--fast"][..], &[]] {
        let run = bench.run(
            "gate",
            args,
            &[
                ("PITO_EMBED_MODELS", "models"),
                ("CARGO_NET_OFFLINE", "true"),
                ("STUB_CARGO_FAIL", "run"),
            ],
        );
        assert_eq!(run.status, Some(7), "{args:?}");
        assert_eq!(run.cargo, [seeding("true", "models")], "{args:?}");
        assert!(!run.stdout.contains("green"), "{args:?}");
    }
}
