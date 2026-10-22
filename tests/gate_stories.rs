#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const LINUX: &str = "x86_64-unknown-linux-gnu";
const MAC: &str = "aarch64-apple-darwin";
const MAC_HASH: &str = "c887d7ec10b06832d95e0fa035a28425794b26e1a5c34804bda7c6b50f7197a3";
const LINUX_FEATURES: &str =
    "__tls,api-17,api-18,copy-dylibs,download-binaries,std,tls-native,webgpu";
const URL: &str = "https://example.invalid/ort/x86_64-unknown-linux-gnu+webgpu.tar.lzma2";
const LIBS: [&str; 2] = ["f:libonnxruntime.a", "f:libwebgpu_dawn.so"];

const FIXTURE: &str = r#"
import hashlib, io, lzma, sys, tarfile
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tar:
    for spec in sys.argv[2:]:
        kind, name = spec.split(":", 1)
        info = tarfile.TarInfo(name)
        data = None
        if kind == "d":
            info.type = tarfile.DIRTYPE
        elif kind == "l":
            info.type = tarfile.SYMTYPE
            info.linkname = "/etc/passwd"
        else:
            data = io.BytesIO(name.encode())
            info.size = len(name)
        tar.addfile(info, data)
packed = lzma.compress(buf.getvalue(), format=lzma.FORMAT_RAW, filters=[{"id": lzma.FILTER_LZMA2, "dict_size": 1 << 16}])
open(sys.argv[1], "wb").write(packed)
print(hashlib.sha256(packed).hexdigest())
"#;

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
    hash: String,
}

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    cargo: Vec<String>,
    curl: Vec<String>,
}

impl Bench {
    fn new(label: &str) -> Self {
        let scratch = scratch_dir(label);
        let bin = scratch.path().join("bin");
        write_exe(
            &bin.join("cargo"),
            "#!/bin/sh\n\
             printf '%s|%s|%s\\n' \"${CARGO_NET_OFFLINE-unset}\" \"${PITO_EMBED_MODELS-unset}\" \"$*\" >> \"$STUB_LOG\"\n\
             case \"$1\" in\n\
             metadata) cat \"$STUB_METADATA\"; exit 0 ;;\n\
             tree) printf '%s\\n' \"$STUB_FEATURES\"; exit 0 ;;\n\
             esac\n\
             if [ \"$1\" = \"${STUB_CARGO_FAIL-none}\" ]; then exit 7; fi\n\
             exit 0\n",
        );
        write_exe(
            &bin.join("rustc"),
            "#!/bin/sh\nprintf 'rustc 1.97.0\\nhost: %s\\n' \"$STUB_HOST\"\n",
        );
        write_exe(
            &bin.join("uname"),
            "#!/bin/sh\nprintf '%s\\n' \"$STUB_OS\"\n",
        );
        write_exe(
            &bin.join("curl"),
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> \"$STUB_CURL_LOG\"\n\
             if [ -n \"${STUB_RACE-}\" ]; then mkdir -p \"$STUB_RACE\"; printf theirs > \"$STUB_RACE/${STUB_RACE_FILE:-libonnxruntime.a}\"; fi\n\
             if [ -z \"$STUB_ARCHIVE\" ]; then echo 'curl: (6) Could not resolve host: example.invalid' >&2; exit 6; fi\n\
             out=''\n\
             while [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then out=$2; shift; fi; shift; done\n\
             cp \"$STUB_ARCHIVE\" \"$out\"\n",
        );
        let pkg = scratch.path().join("registry/ort-sys-2.0.0-rc.13");
        std::fs::create_dir_all(pkg.join("build/download")).expect("fake ort-sys is creatable");
        std::fs::write(pkg.join("Cargo.toml"), "").expect("fake manifest is writable");
        let id = "registry+https://github.com/rust-lang/crates.io-index#ort-sys@2.0.0-rc.13";
        let metadata = format!(
            r#"{{"packages":[{{"name":"embed","version":"1.0.4","id":"path+file:///w#embed@1.0.4","manifest_path":"/w/Cargo.toml"}},{{"name":"ort-sys","version":"2.0.0-rc.13","id":"{id}","manifest_path":"{}"}}],"resolve":{{"nodes":[{{"id":"{id}","dependencies":[],"deps":[]}}],"root":"path+file:///w#embed@1.0.4"}},"version":1}}"#,
            pkg.join("Cargo.toml").display()
        );
        std::fs::write(scratch.path().join("metadata.json"), metadata)
            .expect("metadata is writable");
        let mut bench = Self {
            scratch,
            hash: String::new(),
        };
        bench.hash = bench.archive(&LIBS);
        bench.dist(&bench.hash.clone());
        bench
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.scratch.path().join(rel)
    }

    fn archive(&self, entries: &[&str]) -> String {
        let out = Command::new("python3")
            .args(["-I", "-c", FIXTURE])
            .arg(self.path("archive.lzma2"))
            .args(entries)
            .output()
            .expect("python3 builds the fixture archive");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn dist(&self, linux_webgpu_hash: &str) {
        let zero = "0".repeat(64);
        let ones = "1".repeat(64);
        let table = format!(
            "target\tfeature_set\turl\tsha256_hash\n\
             # pinned builds\n\
             {LINUX}\tnone\thttps://example.invalid/ort/none\t{zero}\n\
             {LINUX}\twebgpu\t{URL}\t{linux_webgpu_hash}\n\
             {MAC}\tcoreml\thttps://example.invalid/ort/mac\t{ones}\n\
             {MAC}\tcoreml,webgpu\thttps://example.invalid/ort/mac-webgpu\t{MAC_HASH}\n"
        );
        std::fs::write(
            self.path("registry/ort-sys-2.0.0-rc.13/build/download/dist.tsv"),
            table,
        )
        .expect("dist table is writable");
    }

    fn default_dest(&self) -> PathBuf {
        self.path("home/.cache/ort.pyke.io/dfbin")
            .join(LINUX)
            .join(&self.hash)
    }

    fn seed(&self, dest: &Path) {
        std::fs::create_dir_all(dest).expect("cache folder is creatable");
        std::fs::write(dest.join("libonnxruntime.a"), "seeded").expect("library is writable");
    }

    fn run(&self, script: &str, args: &[&str], env: &[(&str, &str)]) -> Run {
        let log = self.path("cargo.log");
        let curl_log = self.path("curl.log");
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(&curl_log);
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
        .env("STUB_LOG", &log)
        .env("STUB_CURL_LOG", &curl_log)
        .env("STUB_METADATA", self.path("metadata.json"))
        .env("STUB_FEATURES", LINUX_FEATURES)
        .env("STUB_HOST", LINUX)
        .env("STUB_OS", "Linux")
        .env("STUB_ARCHIVE", self.path("archive.lzma2"));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("bash runs the script");
        let lines = |p: &Path| {
            std::fs::read_to_string(p)
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default()
        };
        Run {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            cargo: lines(&log),
            curl: lines(&curl_log),
        }
    }

    fn leftovers(&self, dest: &Path) -> Vec<String> {
        std::fs::read_dir(dest.parent().unwrap())
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
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

fn fast_offline(net: &str) -> Vec<String> {
    vec![
        format!("{net}|models|fmt --all --check"),
        "unset|models|clippy --offline --workspace --all-targets -- -D warnings".into(),
        "unset|unset|nextest run --offline --workspace --no-tests=pass".into(),
        "unset|models|test --offline --workspace --doc".into(),
    ]
}

fn full_offline(net: &str) -> Vec<String> {
    vec![
        format!("{net}|models|fmt --all --check"),
        "unset|models|clippy --offline --workspace --all-targets --locked -- -D warnings".into(),
        "unset|models|test --offline --workspace --doc --locked".into(),
        "unset|models|run --offline --release --locked --example search -- a hidden area".into(),
        "unset|models|nextest run --offline --workspace --locked --no-tests=pass".into(),
    ]
}

fn seeding_calls(net: &str, models: &str) -> Vec<String> {
    vec![
        format!(
            "{net}|{models}|metadata --format-version 1 --locked --filter-platform {LINUX} --offline"
        ),
        format!(
            "{net}|{models}|tree --offline --locked --workspace --target {LINUX} -e normal -i ort-sys --depth 0 --prefix none -f {{f}}"
        ),
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
        assert!(run.curl.is_empty());
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
        let _ = std::fs::remove_dir_all(bench.path("home"));
        let env = [("PITO_EMBED_MODELS", "models"), ("CARGO_NET_OFFLINE", net)];
        for (args, steps, green) in [
            (&["--fast"][..], fast_offline(net), "fast gate: green"),
            (&[], full_offline(net), "gate: green"),
        ] {
            let run = bench.run("gate", args, &env);
            assert_eq!(run.status, Some(0), "{net} {args:?}: {}", run.stderr);
            let mut expected = seeding_calls(net, "models");
            expected.extend(steps);
            assert_eq!(run.cargo, expected, "{net} {args:?}");
            let lines: Vec<_> = run.stdout.lines().collect();
            assert_eq!(
                lines.first().copied(),
                Some(bench.default_dest().to_str().unwrap())
            );
            assert_eq!(lines.last().copied(), Some(green));
        }
        let run = bench.run("gate", &["--fast"], &env);
        assert!(run.curl.is_empty(), "a seeded cache is not fetched again");
        assert!(bench.default_dest().join("libonnxruntime.a").is_file());
    }
}

#[test]
fn the_offline_gate_stops_before_any_step_when_ort_cache_refuses() {
    let bench = Bench::new("gate-offline-refused-");
    let run = bench.run(
        "gate",
        &["--fast"],
        &[
            ("CARGO_NET_OFFLINE", "true"),
            ("STUB_HOST", "sparc-sun-solaris"),
        ],
    );
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        "bin/ort-cache: no ONNX Runtime build for sparc-sun-solaris\n"
    );
    assert!(
        run.cargo
            .iter()
            .all(|c| c.contains("|metadata ") || c.contains("|tree ")),
        "{:?}",
        run.cargo
    );
    assert!(!run.stdout.contains("green"));
}

#[test]
fn ort_cache_refuses_bad_arguments_as_clap_does() {
    let bench = Bench::new("ort-args-");
    let usage = |first: &str| clap_shape("bin/ort-cache", "[OPTIONS]", first);
    let missing = "error: a value is required for '--target <TARGET>' but none was supplied\n\nFor more information, try '--help'.\n";
    for (args, stderr) in [
        (
            &["--bogus", "--help"][..],
            usage("error: unexpected argument '--bogus' found"),
        ),
        (
            &["stray"],
            usage("error: unexpected argument 'stray' found"),
        ),
        (
            &["--check", "--check"],
            usage("error: the argument '--check' cannot be used multiple times"),
        ),
        (
            &["--target", "a", "--target=b"],
            usage("error: the argument '--target <TARGET>' cannot be used multiple times"),
        ),
        (&["--target"], missing.to_string()),
        (&["--target", "--check"], missing.to_string()),
        (&["--target="], missing.to_string()),
    ] {
        let run = bench.run("ort-cache", args, &[]);
        assert_eq!(run.status, Some(2), "{args:?}");
        assert!(run.stdout.is_empty(), "{args:?}");
        assert_eq!(run.stderr, stderr, "{args:?}");
        assert!(run.cargo.is_empty(), "{args:?}");
    }
    for args in [&["-h"][..], &["--check", "--help", "--bogus"]] {
        let run = bench.run("ort-cache", args, &[]);
        assert_eq!(run.status, Some(0), "{args:?}");
        assert!(run.stdout.contains("Usage: bin/ort-cache [OPTIONS]"));
        assert!(run.stderr.is_empty() && run.cargo.is_empty(), "{args:?}");
    }
}

#[test]
fn ort_cache_check_prints_the_folder_and_says_whether_it_holds_the_library() {
    let bench = Bench::new("ort-check-");
    let dest = bench.default_dest();
    let run = bench.run("ort-cache", &["--check"], &[]);
    assert_eq!(run.status, Some(1), "{}", run.stderr);
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert!(run.stderr.is_empty() && run.curl.is_empty());
    assert!(!dest.exists());
    assert_eq!(
        run.cargo,
        [
            format!("unset|unset|metadata --format-version 1 --locked --filter-platform {LINUX}"),
            format!(
                "unset|unset|tree --locked --workspace --target {LINUX} -e normal -i ort-sys --depth 0 --prefix none -f {{f}}"
            ),
        ]
    );
    std::fs::create_dir_all(&dest).unwrap();
    assert_eq!(bench.run("ort-cache", &["--check"], &[]).status, Some(1));
    bench.seed(&dest);
    let run = bench.run("ort-cache", &["--check"], &[]);
    assert_eq!(run.status, Some(0));
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
}

#[test]
fn ort_cache_resolves_the_folder_where_ort_sys_looks_on_each_os() {
    let bench = Bench::new("ort-dirs-");
    let home = bench.path("home");
    let mac = format!("dfbin/{MAC}/{MAC_HASH}");
    let linux = format!("dfbin/{LINUX}/{}", bench.hash);
    let mac_features = [("STUB_FEATURES", "api-17,coreml,download-binaries,webgpu")];
    let xdg = bench.path("xdg");
    let ort = bench.path("ort");
    for (os, env, target, expected) in [
        (
            "Linux",
            vec![],
            LINUX,
            home.join(".cache/ort.pyke.io").join(&linux),
        ),
        (
            "Linux",
            vec![("XDG_CACHE_HOME", xdg.to_str().unwrap())],
            LINUX,
            xdg.join(&linux),
        ),
        (
            "Linux",
            vec![("XDG_CACHE_HOME", "relative/cache")],
            LINUX,
            home.join(".cache/ort.pyke.io").join(&linux),
        ),
        (
            "Linux",
            vec![
                ("ORT_CACHE_DIR", ort.to_str().unwrap()),
                ("XDG_CACHE_HOME", xdg.to_str().unwrap()),
            ],
            LINUX,
            ort.join(&linux),
        ),
        (
            "Darwin",
            vec![("XDG_CACHE_HOME", xdg.to_str().unwrap())],
            MAC,
            home.join("Library/Caches/ort.pyke.io").join(&mac),
        ),
        (
            "Darwin",
            vec![("ORT_CACHE_DIR", ort.to_str().unwrap())],
            MAC,
            ort.join(&mac),
        ),
    ] {
        let mut env = env;
        env.push(("STUB_OS", os));
        if target == MAC {
            env.extend(mac_features);
        }
        let run = bench.run("ort-cache", &["--check", "--target", target], &env);
        assert_eq!(run.status, Some(1), "{os} {env:?}: {}", run.stderr);
        assert_eq!(
            run.stdout,
            format!("{}\n", expected.display()),
            "{os} {env:?}"
        );
        assert!(run.cargo[0].ends_with(&format!("--filter-platform {target}")));
    }
    for bad in ["", "relative/ort"] {
        let run = bench.run("ort-cache", &["--check"], &[("ORT_CACHE_DIR", bad)]);
        assert_eq!(run.status, Some(1));
        assert_eq!(
            run.stderr,
            "bin/ort-cache: ORT_CACHE_DIR must be an absolute path\n"
        );
        assert!(run.stdout.is_empty());
    }
    let run = bench.run("ort-cache", &["--check"], &[("STUB_OS", "FreeBSD")]);
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        "bin/ort-cache: ort-sys keeps no cache folder this script knows on FreeBSD\n"
    );
}

#[test]
fn ort_cache_fetches_checks_and_extracts_the_pinned_archive_once() {
    let bench = Bench::new("ort-fetch-");
    let ort = bench.path("ort");
    let xdg = bench.path("xdg");
    for (env, cache) in [
        (
            vec![
                ("ORT_CACHE_DIR", ort.to_str().unwrap()),
                ("CARGO_NET_OFFLINE", "true"),
            ],
            ort.clone(),
        ),
        (vec![("XDG_CACHE_HOME", xdg.to_str().unwrap())], xdg.clone()),
    ] {
        let dest = cache.join("dfbin").join(LINUX).join(&bench.hash);
        let run = bench.run("ort-cache", &[], &env);
        assert_eq!(run.status, Some(0), "{env:?}: {}", run.stderr);
        assert_eq!(run.stdout, format!("{}\n", dest.display()));
        assert!(run.stderr.is_empty(), "{}", run.stderr);
        assert_eq!(run.curl.len(), 1);
        assert!(
            run.curl[0].starts_with("--proto =https "),
            "{}",
            run.curl[0]
        );
        assert!(run.curl[0].contains(" --fail "), "{}", run.curl[0]);
        assert!(
            run.curl[0].contains(" --connect-timeout 30 --max-time 1800 "),
            "{}",
            run.curl[0]
        );
        assert!(run.curl[0].ends_with(URL), "{}", run.curl[0]);
        for lib in ["libonnxruntime.a", "libwebgpu_dawn.so"] {
            assert_eq!(std::fs::read_to_string(dest.join(lib)).unwrap(), lib);
        }
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 2);
        assert_eq!(bench.leftovers(&dest), std::slice::from_ref(&bench.hash));
        let again = bench.run("ort-cache", &[], &env);
        assert_eq!(again.status, Some(0));
        assert_eq!(again.stdout, run.stdout);
        assert!(again.curl.is_empty());
        assert_eq!(bench.run("ort-cache", &["--check"], &env).status, Some(0));
    }
}

fn sha256(path: &Path) -> String {
    let out = Command::new("python3")
        .args([
            "-I",
            "-c",
            "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())",
        ])
        .arg(path)
        .output()
        .expect("python3 hashes the file");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn path_without_xz(bench: &Bench) -> Option<String> {
    let tools = bench.path("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let sum = if find_tool("sha256sum").is_some() {
        "sha256sum"
    } else {
        "shasum"
    };
    for tool in [
        "bash", "python3", "dirname", "sed", "tr", "mkdir", "mktemp", "rm", "cat", "cp", sum,
    ] {
        let Some(found) = find_tool(tool) else {
            eprintln!("skipped: no {tool} on PATH");
            return None;
        };
        let link = tools.join(tool);
        if !link.exists() {
            std::os::unix::fs::symlink(found, link).unwrap();
        }
    }
    let path = std::env::join_paths([bench.path("bin"), tools]).unwrap();
    Some(path.to_str().unwrap().to_string())
}

#[test]
fn ort_cache_extracts_with_python_when_the_host_has_no_xz() {
    let bench = Bench::new("ort-noxz-");
    let Some(path) = path_without_xz(&bench) else {
        return;
    };
    let run = bench.run("ort-cache", &[], &[("PATH", &path)]);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    let dest = bench.default_dest();
    assert_eq!(
        std::fs::read_to_string(dest.join("libonnxruntime.a")).unwrap(),
        "libonnxruntime.a"
    );
}

#[test]
fn ort_cache_leaves_nothing_behind_on_a_wrong_hash_or_a_failed_download() {
    let bench = Bench::new("ort-badfetch-");
    let dest = bench.default_dest();
    bench.dist(&"a".repeat(64));
    let wrong = dest.parent().unwrap().join("a".repeat(64));
    let run = bench.run("ort-cache", &[], &[]);
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "bin/ort-cache: {URL} has sha256 {}, not the pinned {}\n",
            bench.hash,
            "a".repeat(64)
        )
    );
    assert!(run.stdout.is_empty());
    assert!(!wrong.exists());
    assert!(
        bench.leftovers(&dest).is_empty(),
        "{:?}",
        bench.leftovers(&dest)
    );
    bench.dist(&bench.hash);
    let run = bench.run("ort-cache", &[], &[("STUB_ARCHIVE", "")]);
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "bin/ort-cache: could not download {URL}: curl: (6) Could not resolve host: example.invalid\n"
        )
    );
    assert!(!dest.exists());
    assert!(
        bench.leftovers(&dest).is_empty(),
        "{:?}",
        bench.leftovers(&dest)
    );
}

#[test]
fn ort_cache_refuses_a_target_or_feature_set_it_has_no_build_for() {
    let bench = Bench::new("ort-nodist-");
    for (args, features, stderr) in [
        (
            &["--target", "sparc-sun-solaris"][..],
            LINUX_FEATURES,
            "no ONNX Runtime build for sparc-sun-solaris",
        ),
        (
            &[],
            "api-17,coreml,directml,webgpu",
            "no ONNX Runtime build for x86_64-unknown-linux-gnu with features coreml,directml,webgpu",
        ),
        (
            &["--target", MAC],
            "api-17,coreml,directml,webgpu",
            "no ONNX Runtime build for aarch64-apple-darwin with features coreml,directml,webgpu",
        ),
        (
            &[],
            "api-17,cuda",
            "the cuda and tensorrt features of ort-sys are not supported",
        ),
        (
            &[],
            "tensorrt,webgpu",
            "the cuda and tensorrt features of ort-sys are not supported",
        ),
    ] {
        let run = bench.run("ort-cache", args, &[("STUB_FEATURES", features)]);
        assert_eq!(run.status, Some(1), "{args:?} {features}");
        assert_eq!(
            run.stderr,
            format!("bin/ort-cache: {stderr}\n"),
            "{args:?} {features}"
        );
        assert!(run.stdout.is_empty() && run.curl.is_empty());
        assert!(!bench.path("home/.cache").exists());
    }
    let run = bench.run(
        "ort-cache",
        &["--check"],
        &[("STUB_FEATURES", "api-17,std")],
    );
    assert_eq!(run.status, Some(1));
    assert!(
        run.stdout
            .ends_with(&format!("dfbin/{LINUX}/{}\n", "0".repeat(64))),
        "{}",
        run.stdout
    );
}

#[test]
fn ort_cache_refuses_an_archive_that_is_not_flat_plain_files_and_leaves_nothing() {
    let bench = Bench::new("ort-unsafe-");
    let dest = bench.default_dest();
    for (entries, needle) in [
        (
            &["f:libonnxruntime.a", "f:lib/nested.a"][..],
            "holds 'lib/nested.a', which is not a plain file at the top level",
        ),
        (
            &["f:../escape.a", "f:libonnxruntime.a"],
            "holds '../escape.a', which is not a plain file at the top level",
        ),
        (
            &["d:lib", "f:libonnxruntime.a"],
            "holds 'lib', which is not a plain file at the top level",
        ),
        (
            &["l:libonnxruntime.a"],
            "holds 'libonnxruntime.a', which is not a plain file at the top level",
        ),
        (
            &["f:./lib/nested.a", "f:libonnxruntime.a"],
            "holds './lib/nested.a', which is not a plain file at the top level",
        ),
        (
            &["f:./../escape.a", "f:libonnxruntime.a"],
            "holds './../escape.a', which is not a plain file at the top level",
        ),
        (&["f:libwebgpu_dawn.so"], "holds no libonnxruntime.a"),
    ] {
        let hash = bench.archive(entries);
        bench.dist(&hash);
        let dest = dest.parent().unwrap().join(&hash);
        let run = bench.run("ort-cache", &[], &[]);
        assert_eq!(run.status, Some(1), "{entries:?}");
        assert_eq!(
            run.stderr,
            format!("bin/ort-cache: {URL} {needle}\n"),
            "{entries:?}"
        );
        assert!(!dest.exists(), "{entries:?}");
        assert!(
            bench.leftovers(&dest).is_empty(),
            "{entries:?}: {:?}",
            bench.leftovers(&dest)
        );
        assert!(
            !bench.path("escape.a").exists()
                && !bench
                    .path("home/.cache/ort.pyke.io/dfbin/escape.a")
                    .exists()
        );
    }
    std::fs::write(bench.path("archive.lzma2"), "not lzma2 at all").unwrap();
    let hash = sha256(&bench.path("archive.lzma2"));
    bench.dist(&hash);
    let run = bench.run("ort-cache", &[], &[]);
    assert_eq!(run.status, Some(1));
    assert_eq!(run.stderr.lines().count(), 1, "{}", run.stderr);
    assert!(!dest.parent().unwrap().join(&hash).exists());
    assert!(bench.leftovers(&dest).is_empty());
}

#[test]
fn ort_cache_refuses_a_cache_folder_without_the_library_instead_of_fetching() {
    let bench = Bench::new("ort-hollow-");
    let dest = bench.default_dest();
    std::fs::create_dir_all(&dest).unwrap();
    let run = bench.run("ort-cache", &[], &[]);
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "bin/ort-cache: {} exists without libonnxruntime.a; remove it to fetch again\n",
            dest.display()
        )
    );
    assert!(run.curl.is_empty());
}

fn ort_sys_in_registry() -> Option<PathBuf> {
    let lock =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock")).ok()?;
    let version = lock.split("[[package]]").find_map(|p| {
        p.lines().any(|l| l == "name = \"ort-sys\"").then(|| {
            p.lines()
                .find_map(|l| l.strip_prefix("version = \"")?.strip_suffix('"'))
        })?
    })?;
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    std::fs::read_dir(cargo_home.join("registry/src"))
        .ok()?
        .flatten()
        .map(|e| e.path().join(format!("ort-sys-{version}")))
        .find(|p| p.join("build/download/dist.tsv").is_file())
}

#[test]
fn ort_cache_check_on_the_real_lockfile_names_the_pinned_mac_build() {
    if ort_sys_in_registry().is_none() {
        eprintln!("skipped: ort-sys is not in the local registry");
        return;
    }
    let scratch = scratch_dir("ort-real-");
    let mut cmd = Command::new(find_tool("bash").expect("bash is on PATH"));
    cmd.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/ort-cache"))
        .args(["--check", "--target", MAC])
        .env_clear()
        .env("PATH", real_path())
        .env("CARGO_NET_OFFLINE", "true")
        .env("ORT_CACHE_DIR", scratch.path());
    for keep in ["HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
        if let Some(v) = std::env::var_os(keep) {
            cmd.env(keep, v);
        }
    }
    let out = cmd.output().expect("bash runs bin/ort-cache");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let expected = scratch.path().join("dfbin").join(MAC).join(MAC_HASH);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", expected.display())
    );
}

#[test]
fn ort_cache_accepts_entries_written_with_a_leading_dot_slash() {
    let bench = Bench::new("ort-dotslash-");
    let hash = bench.archive(&["f:./libonnxruntime.a", "f:libwebgpu_dawn.so"]);
    bench.dist(&hash);
    let dest = bench.default_dest().parent().unwrap().join(&hash);
    let run = bench.run("ort-cache", &[], &[]);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert_eq!(
        std::fs::read_to_string(dest.join("libonnxruntime.a")).unwrap(),
        "./libonnxruntime.a"
    );
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 2);
}

#[test]
fn ort_cache_keeps_the_folder_another_process_landed_first() {
    let bench = Bench::new("ort-race-");
    let dest = bench.default_dest();
    let race = [("STUB_RACE", dest.to_str().unwrap())];
    let run = bench.run("ort-cache", &[], &race);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert!(run.stderr.is_empty(), "{}", run.stderr);
    assert_eq!(
        std::fs::read_to_string(dest.join("libonnxruntime.a")).unwrap(),
        "theirs"
    );
    assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
    assert_eq!(bench.leftovers(&dest), std::slice::from_ref(&bench.hash));
}

#[test]
fn ort_cache_refuses_when_the_folder_another_process_landed_lacks_the_library() {
    let bench = Bench::new("ort-race-bad-");
    let dest = bench.default_dest();
    let run = bench.run(
        "ort-cache",
        &[],
        &[
            ("STUB_RACE", dest.to_str().unwrap()),
            ("STUB_RACE_FILE", "partial"),
        ],
    );
    assert_eq!(run.status, Some(1));
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    assert_eq!(run.stderr.lines().count(), 1, "{}", run.stderr);
    assert!(
        run.stderr.starts_with(&format!(
            "bin/ort-cache: cannot move the files into {}: ",
            dest.display()
        )),
        "{}",
        run.stderr
    );
    assert!(!dest.join("libonnxruntime.a").exists());
    assert_eq!(bench.leftovers(&dest), std::slice::from_ref(&bench.hash));
}

#[test]
fn ort_cache_refuses_a_stream_cut_before_its_end_marker_with_either_decoder() {
    let bench = Bench::new("ort-cut-");
    let archive = bench.path("archive.lzma2");
    let mut bytes = std::fs::read(&archive).unwrap();
    assert_eq!(
        bytes.pop(),
        Some(0),
        "raw LZMA2 ends with its 0x00 end marker"
    );
    std::fs::write(&archive, bytes).unwrap();
    let hash = sha256(&archive);
    bench.dist(&hash);
    let dest = bench.default_dest().parent().unwrap().join(&hash);
    let mut paths = vec![None];
    if let Some(path) = path_without_xz(&bench) {
        paths.push(Some(path));
    }
    for path in paths {
        let env: Vec<_> = path.as_deref().map(|p| ("PATH", p)).into_iter().collect();
        let run = bench.run("ort-cache", &[], &env);
        assert_eq!(run.status, Some(1), "{path:?}");
        assert_eq!(
            run.stderr,
            format!("bin/ort-cache: {URL} is not a raw LZMA2 stream\n"),
            "{path:?}"
        );
        assert!(!dest.exists(), "{path:?}");
        assert!(bench.leftovers(&dest).is_empty(), "{path:?}");
    }
}
