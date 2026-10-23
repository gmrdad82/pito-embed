#![cfg(unix)]

#[path = "../src/fixture.rs"]
mod fixture;

use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ort_cache::args::Options;
use ort_cache::dirs::{CacheEnv, Os};
use ort_cache::land::Source;
use ort_cache::{Inputs, Outcome};

const LINUX: &str = "x86_64-unknown-linux-gnu";
const MAC: &str = "aarch64-apple-darwin";
const MAC_HASH: &str = "c887d7ec10b06832d95e0fa035a28425794b26e1a5c34804bda7c6b50f7197a3";
const LINUX_FEATURES: &str =
    "__tls,api-17,api-18,copy-dylibs,download-binaries,std,tls-native,webgpu";
const URL: &str = "https://example.invalid/ort/x86_64-unknown-linux-gnu+webgpu.tar.lzma2";

fn scratch_dir(label: &str) -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
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

struct Bench {
    scratch: tempfile::TempDir,
    archive: Vec<u8>,
    hash: String,
}

struct Run {
    status: Option<i32>,
    stdout: String,
    stderr: String,
    cargo: Vec<String>,
}

struct Bytes<'a> {
    data: &'a [u8],
    calls: Cell<usize>,
    then: Box<dyn Fn() + 'a>,
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            calls: Cell::new(0),
            then: Box::new(|| {}),
        }
    }
}

impl Source for Bytes<'_> {
    fn fetch(&self, url: &str, _: &str, into: &Path) -> Result<PathBuf, String> {
        assert_eq!(url, URL);
        self.calls.set(self.calls.get() + 1);
        let path = into.join("archive");
        std::fs::write(&path, self.data).unwrap();
        (self.then)();
        Ok(path)
    }
}

impl Bench {
    fn new(label: &str) -> Self {
        let scratch = scratch_dir(label);
        let root = scratch.path().to_path_buf();
        write_exe(
            &root.join("bin/cargo"),
            &format!(
                "#!/bin/sh\n\
                 printf '%s|%s\\n' \"${{CARGO_NET_OFFLINE-unset}}\" \"$*\" >> '{0}/cargo.log'\n\
                 case \"$1\" in\n\
                 metadata) cat '{0}/metadata.json' || exit 101 ;;\n\
                 tree) cat '{0}/features' ;;\n\
                 *) exit 9 ;;\n\
                 esac\n",
                root.display()
            ),
        );
        write_exe(
            &root.join("bin/rustc"),
            &format!(
                "#!/bin/sh\nprintf 'rustc 1.97.0\\nhost: %s\\n' \"$(cat '{}/host')\"\n",
                root.display()
            ),
        );
        let pkg = root.join("registry/ort-sys-2.0.0-rc.13");
        std::fs::create_dir_all(pkg.join("build/download")).unwrap();
        let metadata = format!(
            r#"{{"packages":[{{"name":"embed","manifest_path":"/w/Cargo.toml"}},{{"name":"ort-sys","manifest_path":"{}"}}],"resolve":null,"version":1}}"#,
            pkg.join("Cargo.toml").display()
        );
        std::fs::write(root.join("metadata.json"), metadata).unwrap();
        std::fs::write(root.join("features"), LINUX_FEATURES).unwrap();
        std::fs::write(root.join("host"), LINUX).unwrap();
        let archive = fixture::lzma2(&fixture::tar(&[
            "f:libonnxruntime.a",
            "f:libwebgpu_dawn.so",
        ]));
        let bench = Self {
            scratch,
            hash: fetch::sha256_hex(&archive),
            archive,
        };
        bench.dist(&bench.hash);
        bench
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.scratch.path().join(rel)
    }

    fn dist(&self, linux_webgpu_hash: &str) {
        let table = format!(
            "target\tfeature_set\turl\tsha256_hash\n\
             # pinned builds\n\
             {LINUX}\tnone\thttps://example.invalid/ort/none\t{}\n\
             {LINUX}\twebgpu\t{URL}\t{linux_webgpu_hash}\n\
             {MAC}\tcoreml,webgpu\thttps://example.invalid/ort/mac-webgpu\t{MAC_HASH}\n",
            "0".repeat(64)
        );
        std::fs::write(
            self.path("registry/ort-sys-2.0.0-rc.13/build/download/dist.tsv"),
            table,
        )
        .unwrap();
    }

    fn default_dest(&self) -> PathBuf {
        self.path("home/.cache/ort.pyke.io/dfbin")
            .join(LINUX)
            .join(&self.hash)
    }

    fn cargo_log(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.path("cargo.log")).unwrap_or_default();
        let _ = std::fs::remove_file(self.path("cargo.log"));
        log.lines().map(str::to_string).collect()
    }

    fn bin(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let _ = std::fs::remove_file(self.path("cargo.log"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ort-cache"));
        cmd.args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("CARGO", self.path("bin/cargo"))
            .env("RUSTC", self.path("bin/rustc"))
            .env("HOME", self.path("home"))
            .env("NO_COLOR", "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("the binary runs");
        Run {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            cargo: self.cargo_log(),
        }
    }

    fn inputs(&self, cache: CacheEnv) -> Inputs {
        Inputs {
            cargo: self.path("bin/cargo").into(),
            rustc: self.path("bin/rustc").into(),
            offline: true,
            os: Os::Linux,
            cache,
        }
    }

    fn home(&self) -> CacheEnv {
        CacheEnv {
            home: Some(self.path("home")),
            ..CacheEnv::default()
        }
    }

    fn lib(&self, source: &dyn Source, cache: CacheEnv) -> Result<Outcome, String> {
        ort_cache::run(&Options::default(), &self.inputs(cache), source)
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

fn clap_shape(first: &str) -> String {
    format!("{first}\n\nUsage: ort-cache [OPTIONS]\n\nFor more information, try '--help'.\n")
}

#[test]
fn bad_arguments_are_refused_as_clap_does_before_cargo_runs() {
    let bench = Bench::new("args-");
    let missing = "error: a value is required for '--target <TARGET>' but none was supplied\n\nFor more information, try '--help'.\n";
    for (args, stderr) in [
        (
            &["--bogus", "--help"][..],
            clap_shape("error: unexpected argument '--bogus' found"),
        ),
        (
            &["stray"],
            clap_shape("error: unexpected argument 'stray' found"),
        ),
        (
            &["--check", "--check"],
            clap_shape("error: the argument '--check' cannot be used multiple times"),
        ),
        (
            &["--target", "a", "--target=b"],
            clap_shape("error: the argument '--target <TARGET>' cannot be used multiple times"),
        ),
        (&["--target"], missing.to_string()),
        (&["--target", "--check"], missing.to_string()),
        (&["--target="], missing.to_string()),
    ] {
        let run = bench.bin(args, &[]);
        assert_eq!(run.status, Some(2), "{args:?}");
        assert!(run.stdout.is_empty(), "{args:?}");
        assert_eq!(run.stderr, stderr, "{args:?}");
        assert!(run.cargo.is_empty(), "{args:?}");
    }
    for args in [&["-h"][..], &["--check", "--help", "--bogus"]] {
        let run = bench.bin(args, &[]);
        assert_eq!(run.status, Some(0), "{args:?}");
        assert!(run.stdout.contains("Usage: ort-cache [OPTIONS]"));
        assert!(run.stderr.is_empty() && run.cargo.is_empty(), "{args:?}");
    }
}

#[test]
fn check_prints_the_folder_and_says_whether_it_holds_the_library() {
    let bench = Bench::new("check-");
    let dest = bench.default_dest();
    let run = bench.bin(&["--check"], &[]);
    assert_eq!(run.status, Some(1), "{}", run.stderr);
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert!(run.stderr.is_empty());
    assert!(!dest.exists());
    assert_eq!(
        run.cargo,
        [
            format!("unset|metadata --format-version 1 --locked --filter-platform {LINUX}"),
            format!(
                "unset|tree --locked --workspace --target {LINUX} -e normal -i ort-sys --depth 0 --prefix none -f {{f}}"
            ),
        ]
    );
    std::fs::create_dir_all(&dest).unwrap();
    assert_eq!(bench.bin(&["--check"], &[]).status, Some(1));
    std::fs::write(dest.join("libonnxruntime.a"), "seeded").unwrap();
    let run = bench.bin(&["--check"], &[("CARGO_NET_OFFLINE", "TRUE")]);
    assert_eq!(run.status, Some(0));
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert!(run.cargo[0].ends_with(&format!("--filter-platform {LINUX} --offline")));
    assert!(run.cargo[1].starts_with("TRUE|tree --offline --locked "));
}

#[test]
fn an_existing_cache_is_printed_and_nothing_is_fetched() {
    let bench = Bench::new("existing-");
    let dest = bench.default_dest();
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("libonnxruntime.a"), "seeded").unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(run.status, Some(0), "{}", run.stderr);
    assert_eq!(run.stdout, format!("{}\n", dest.display()));
    assert!(run.stderr.is_empty());
    assert_eq!(
        std::fs::read_to_string(dest.join("libonnxruntime.a")).unwrap(),
        "seeded"
    );
    let source = Bytes::new(&bench.archive);
    assert_eq!(
        bench.lib(&source, bench.home()).unwrap(),
        Outcome::Ready(dest)
    );
    assert_eq!(source.calls.get(), 0);
}

#[test]
fn a_cache_folder_without_the_library_is_refused_instead_of_fetched() {
    let bench = Bench::new("hollow-");
    let dest = bench.default_dest();
    std::fs::create_dir_all(&dest).unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(run.status, Some(1));
    assert_eq!(
        run.stderr,
        format!(
            "ort-cache: {} exists without libonnxruntime.a; remove it to fetch again\n",
            dest.display()
        )
    );
    assert!(run.stdout.is_empty());
}

#[test]
fn a_target_or_feature_set_with_no_build_is_refused_in_one_line() {
    let bench = Bench::new("nodist-");
    for (args, features, why) in [
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
            &[],
            "api-17,cuda",
            "the cuda and tensorrt features of ort-sys are not supported",
        ),
    ] {
        std::fs::write(bench.path("features"), features).unwrap();
        let run = bench.bin(args, &[]);
        assert_eq!(run.status, Some(1), "{args:?}");
        assert_eq!(run.stderr, format!("ort-cache: {why}\n"), "{args:?}");
        assert!(run.stdout.is_empty());
        assert!(!bench.path("home").exists());
    }
    std::fs::write(bench.path("host"), "sparc-sun-solaris").unwrap();
    std::fs::write(bench.path("features"), LINUX_FEATURES).unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(
        run.stderr,
        "ort-cache: no ONNX Runtime build for sparc-sun-solaris\n"
    );
    assert!(run.cargo[0].ends_with("--filter-platform sparc-sun-solaris"));
}

#[test]
fn a_graph_a_table_or_a_cache_setting_it_cannot_use_is_refused_in_one_line() {
    let bench = Bench::new("unusable-");
    std::fs::rename(bench.path("metadata.json"), bench.path("gone.json")).unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(run.status, Some(1));
    assert!(
        run.stderr
            .ends_with(&format!("ort-cache: cargo metadata failed for {LINUX}\n")),
        "{}",
        run.stderr
    );
    std::fs::write(
        bench.path("metadata.json"),
        r#"{"packages":[],"resolve":null,"version":1}"#,
    )
    .unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(
        run.stderr,
        format!("ort-cache: the dependency graph holds 0 ort-sys packages for {LINUX}, not one\n")
    );
    std::fs::rename(bench.path("gone.json"), bench.path("metadata.json")).unwrap();
    let table = bench.path("registry/ort-sys-2.0.0-rc.13/build/download/dist.tsv");
    std::fs::write(&table, format!("header\n{LINUX}\twebgpu\t{URL}\n")).unwrap();
    let run = bench.bin(&[], &[]);
    assert_eq!(
        run.stderr,
        format!(
            "ort-cache: {} has a malformed row for {LINUX}\n",
            table.display()
        )
    );
    std::fs::remove_file(&table).unwrap();
    let run = bench.bin(&[], &[]);
    assert!(
        run.stderr
            .starts_with(&format!("ort-cache: cannot read {}: ", table.display())),
        "{}",
        run.stderr
    );
    bench.dist(&bench.hash);
    for bad in ["", "relative/ort"] {
        let run = bench.bin(&["--check"], &[("ORT_CACHE_DIR", bad)]);
        assert_eq!(run.status, Some(1));
        assert_eq!(
            run.stderr,
            "ort-cache: ORT_CACHE_DIR must be an absolute path\n"
        );
        assert!(run.stdout.is_empty());
    }
    let ort = bench.path("ort");
    let run = bench.bin(
        &["--check"],
        &[
            ("ORT_CACHE_DIR", ort.to_str().unwrap()),
            ("XDG_CACHE_HOME", "/elsewhere"),
        ],
    );
    assert_eq!(
        run.stdout,
        format!(
            "{}\n",
            ort.join("dfbin").join(LINUX).join(&bench.hash).display()
        )
    );
}

#[test]
fn a_good_archive_lands_where_ort_sys_looks_once() {
    let bench = Bench::new("land-");
    let xdg = bench.path("xdg");
    let ort = bench.path("ort");
    for (cache, root) in [
        (bench.home(), bench.path("home/.cache/ort.pyke.io")),
        (
            CacheEnv {
                xdg_cache_home: Some(xdg.clone().into()),
                ..bench.home()
            },
            xdg.clone(),
        ),
        (
            CacheEnv {
                ort_cache_dir: Some(ort.clone().into()),
                ..bench.home()
            },
            ort.clone(),
        ),
    ] {
        let dest = root.join("dfbin").join(LINUX).join(&bench.hash);
        let source = Bytes::new(&bench.archive);
        assert_eq!(
            bench.lib(&source, cache.clone()).unwrap(),
            Outcome::Ready(dest.clone())
        );
        assert_eq!(source.calls.get(), 1);
        for lib in ["libonnxruntime.a", "libwebgpu_dawn.so"] {
            assert_eq!(std::fs::read_to_string(dest.join(lib)).unwrap(), lib);
        }
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 2);
        assert_eq!(bench.leftovers(&dest), std::slice::from_ref(&bench.hash));
        assert_eq!(bench.lib(&source, cache).unwrap(), Outcome::Ready(dest));
        assert_eq!(source.calls.get(), 1, "a filled cache is not fetched again");
        let log = bench.cargo_log();
        assert!(log.iter().all(|l| l.contains("--offline")), "{log:?}");
    }
    if cfg!(target_os = "linux") {
        let run = bench.bin(&["--check"], &[("XDG_CACHE_HOME", xdg.to_str().unwrap())]);
        assert_eq!(
            run.status,
            Some(0),
            "the binary finds what the library landed"
        );
    }
}

#[test]
fn a_wrong_sha256_leaves_nothing_behind() {
    let bench = Bench::new("badhash-");
    let pinned = "a".repeat(64);
    bench.dist(&pinned);
    let source = Bytes::new(&bench.archive);
    assert_eq!(
        bench.lib(&source, bench.home()).unwrap_err(),
        format!("{URL} has sha256 {}, not the pinned {pinned}", bench.hash)
    );
    assert!(!bench.path("home").exists());
}

#[test]
fn a_truncated_stream_leaves_nothing_behind() {
    let bench = Bench::new("cut-");
    let mut cut = bench.archive.clone();
    assert_eq!(cut.pop(), Some(0));
    bench.dist(&fetch::sha256_hex(&cut));
    assert_eq!(
        bench.lib(&Bytes::new(&cut), bench.home()).unwrap_err(),
        format!("{URL} is not a raw LZMA2 stream")
    );
    assert!(!bench.path("home").exists());
}

#[test]
fn an_archive_that_is_not_flat_plain_files_leaves_nothing_behind() {
    let bench = Bench::new("unsafe-");
    for (specs, why) in [
        (
            &["f:libonnxruntime.a", "f:lib/nested.a"][..],
            "holds 'lib/nested.a', which is not a plain file at the top level",
        ),
        (
            &["f:../escape.a"],
            "holds '../escape.a', which is not a plain file at the top level",
        ),
        (
            &["l:libonnxruntime.a"],
            "holds 'libonnxruntime.a', which is not a plain file at the top level",
        ),
        (&["f:libwebgpu_dawn.so"], "holds no libonnxruntime.a"),
        (&[], "is an empty archive"),
    ] {
        let data = fixture::lzma2(&fixture::tar(specs));
        bench.dist(&fetch::sha256_hex(&data));
        assert_eq!(
            bench.lib(&Bytes::new(&data), bench.home()).unwrap_err(),
            format!("{URL} {why}")
        );
        assert!(!bench.path("home").exists(), "{specs:?}");
        assert!(!bench.path("escape.a").exists());
    }
}

#[test]
fn the_folder_another_process_landed_first_is_kept_when_it_holds_the_library() {
    let bench = Bench::new("race-");
    let dest = bench.default_dest();
    for (file, kept) in [("libonnxruntime.a", true), ("partial", false)] {
        let _ = std::fs::remove_dir_all(bench.path("home"));
        let mut source = Bytes::new(&bench.archive);
        let theirs = dest.clone();
        source.then = Box::new(move || {
            std::fs::create_dir_all(&theirs).unwrap();
            std::fs::write(theirs.join(file), "theirs").unwrap();
        });
        let got = bench.lib(&source, bench.home());
        if kept {
            assert_eq!(got.unwrap(), Outcome::Ready(dest.clone()));
        } else {
            let err = got.unwrap_err();
            assert!(
                err.starts_with(&format!("cannot move the files into {}: ", dest.display())),
                "{err}"
            );
            assert!(!dest.join("libonnxruntime.a").exists());
        }
        assert_eq!(std::fs::read_to_string(dest.join(file)).unwrap(), "theirs");
        assert_eq!(std::fs::read_dir(&dest).unwrap().count(), 1);
        assert_eq!(bench.leftovers(&dest), std::slice::from_ref(&bench.hash));
    }
}

fn ort_sys_available(root: &Path) -> bool {
    let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
        return false;
    };
    let Some(version) = lock.split("[[package]]").find_map(|p| {
        p.lines().any(|l| l == "name = \"ort-sys\"").then(|| {
            p.lines()
                .find_map(|l| l.strip_prefix("version = \"")?.strip_suffix('"'))
        })?
    }) else {
        return false;
    };
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".cargo")));
    let registry = cargo_home
        .and_then(|h| std::fs::read_dir(h.join("registry/src")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join(format!("ort-sys-{version}")));
    let vendor = ["vendor/ort-sys", &format!("vendor/ort-sys-{version}")].map(|v| root.join(v));
    registry
        .chain(vendor)
        .any(|p| p.join("build/download/dist.tsv").is_file())
}

#[test]
fn check_on_the_real_lockfile_names_the_pinned_mac_build() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !ort_sys_available(&root) {
        eprintln!("skipped: ort-sys is not in the registry or a vendor folder");
        return;
    }
    let scratch = scratch_dir("real-");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ort-cache"));
    cmd.args(["--check", "--target", MAC])
        .current_dir(&root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(
            "CARGO",
            std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        )
        .env("CARGO_NET_OFFLINE", "true")
        .env("ORT_CACHE_DIR", scratch.path());
    for keep in ["HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
        if let Some(v) = std::env::var_os(keep) {
            cmd.env(keep, v);
        }
    }
    let out = cmd.output().expect("the binary runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr
            .lines()
            .all(|l| l.trim_start().starts_with("Blocking waiting for file lock")),
        "{stderr}"
    );
    let expected = scratch.path().join("dfbin").join(MAC).join(MAC_HASH);
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", expected.display())
    );
}
