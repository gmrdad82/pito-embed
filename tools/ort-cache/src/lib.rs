pub mod args;
pub mod dirs;
pub mod dist;
pub mod land;
pub mod ustar;

#[cfg(test)]
mod fixture;

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use args::Options;
use dirs::{CacheEnv, Os};
use land::Source;

pub struct Inputs {
    pub cargo: OsString,
    pub rustc: OsString,
    pub offline: bool,
    pub os: Os,
    pub cache: CacheEnv,
}

impl Inputs {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        Self {
            cargo: var("CARGO").unwrap_or_else(|| "cargo".into()),
            rustc: var("RUSTC").unwrap_or_else(|| "rustc".into()),
            offline: is_offline(std::env::var_os("CARGO_NET_OFFLINE").as_deref()),
            os: Os::current(),
            cache: CacheEnv {
                ort_cache_dir: std::env::var_os("ORT_CACHE_DIR"),
                xdg_cache_home: std::env::var_os("XDG_CACHE_HOME"),
                home: std::env::home_dir(),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ready(PathBuf),
    Missing(PathBuf),
}

pub fn is_offline(value: Option<&OsStr>) -> bool {
    value
        .and_then(OsStr::to_str)
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn metadata_args(target: &str, offline: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--filter-platform",
        target,
    ]
    .map(String::from)
    .into();
    if offline {
        args.push("--offline".into());
    }
    args
}

pub fn tree_args(target: &str, offline: bool) -> Vec<String> {
    let mut args = vec!["tree".to_string()];
    if offline {
        args.push("--offline".into());
    }
    args.extend(
        [
            "--locked",
            "--workspace",
            "--target",
            target,
            "-e",
            "normal",
            "-i",
            "ort-sys",
            "--depth",
            "0",
            "--prefix",
            "none",
            "-f",
            "{f}",
        ]
        .map(String::from),
    );
    args
}

fn stdout_of(program: &OsStr, args: &[String]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn host_of(rustc_vv: &str) -> Option<String> {
    rustc_vv
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(String::from)
}

pub fn ort_sys_dir(metadata: &str, target: &str) -> Result<PathBuf, String> {
    let json: serde_json::Value = serde_json::from_str(metadata)
        .map_err(|_| format!("cargo metadata gave no JSON for {target}"))?;
    let dirs: BTreeSet<PathBuf> = json["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["name"] == "ort-sys")
        .filter_map(|p| p["manifest_path"].as_str())
        .filter_map(|m| Path::new(m).parent().map(Path::to_path_buf))
        .collect();
    match dirs.len() {
        1 => Ok(dirs.into_iter().next().expect("one folder")),
        n => Err(format!(
            "the dependency graph holds {n} ort-sys packages for {target}, not one"
        )),
    }
}

pub fn run(opts: &Options, inputs: &Inputs, source: &dyn Source) -> Result<Outcome, String> {
    let target = match &opts.target {
        Some(t) => t.clone(),
        None => stdout_of(&inputs.rustc, &["-vV".into()])
            .as_deref()
            .and_then(host_of)
            .ok_or("rustc -vV names no host target")?,
    };
    let metadata = stdout_of(&inputs.cargo, &metadata_args(&target, inputs.offline))
        .ok_or_else(|| format!("cargo metadata failed for {target}"))?;
    let pkg = ort_sys_dir(&metadata, &target)?;
    let features = stdout_of(&inputs.cargo, &tree_args(&target, inputs.offline))
        .ok_or_else(|| format!("cargo tree failed for {target}"))?;
    let want = dist::wanted(&features)?;
    let table = pkg.join("build/download/dist.tsv");
    let text = std::fs::read_to_string(&table)
        .map_err(|e| format!("cannot read {}: {e}", table.display()))?;
    let dist = dist::resolve(&text, &table, &target, &want)?;
    let root = dirs::cache_root(&inputs.os, &inputs.cache)?;
    let dest = dirs::dest(&root, &target, &dist.sha256);
    let lib = dirs::library(&target);
    let ready = dest.join(lib).is_file();
    if opts.check || ready {
        return Ok(if ready {
            Outcome::Ready(dest)
        } else {
            Outcome::Missing(dest)
        });
    }
    if dest.exists() {
        return Err(format!(
            "{} exists without {lib}; remove it to fetch again",
            dest.display()
        ));
    }
    land::land(source, &dist, &dest, lib)?;
    Ok(Outcome::Ready(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_or_true_in_any_case_is_offline() {
        for v in ["1", "true", "TRUE", "True"] {
            assert!(is_offline(Some(OsStr::new(v))), "{v}");
        }
        for v in ["0", "false", "yes", "", " 1"] {
            assert!(!is_offline(Some(OsStr::new(v))), "{v}");
        }
        assert!(!is_offline(None));
    }

    #[test]
    fn cargo_is_asked_exactly_as_ort_sys_resolves() {
        assert_eq!(
            metadata_args("t", false).join(" "),
            "metadata --format-version 1 --locked --filter-platform t"
        );
        assert_eq!(
            metadata_args("t", true).join(" "),
            "metadata --format-version 1 --locked --filter-platform t --offline"
        );
        assert_eq!(
            tree_args("t", true).join(" "),
            "tree --offline --locked --workspace --target t -e normal -i ort-sys --depth 0 --prefix none -f {f}"
        );
        assert_eq!(tree_args("t", false)[1], "--locked");
    }

    #[test]
    fn the_host_comes_from_rustc_vv() {
        assert_eq!(
            host_of("rustc 1.97.0\nbinary: rustc\nhost: aarch64-apple-darwin\nrelease: 1.97.0\n")
                .as_deref(),
            Some("aarch64-apple-darwin")
        );
        assert_eq!(host_of("rustc 1.97.0\n"), None);
        assert_eq!(host_of("host: \n"), None);
    }

    #[test]
    fn the_graph_must_hold_exactly_one_ort_sys_folder() {
        let one = r#"{"packages":[{"name":"embed","manifest_path":"/w/Cargo.toml"},{"name":"ort-sys","manifest_path":"/r/ort-sys-2.0.0-rc.13/Cargo.toml"}]}"#;
        assert_eq!(
            ort_sys_dir(one, "t").unwrap(),
            Path::new("/r/ort-sys-2.0.0-rc.13")
        );
        let none =
            r#"{"packages":[{"name":"embed","manifest_path":"/w/Cargo.toml"}],"resolve":null}"#;
        assert_eq!(
            ort_sys_dir(none, "t").unwrap_err(),
            "the dependency graph holds 0 ort-sys packages for t, not one"
        );
        let two = r#"{"packages":[{"name":"ort-sys","manifest_path":"/a/Cargo.toml"},{"name":"ort-sys","manifest_path":"/b/Cargo.toml"}]}"#;
        assert_eq!(
            ort_sys_dir(two, "t").unwrap_err(),
            "the dependency graph holds 2 ort-sys packages for t, not one"
        );
        assert_eq!(
            ort_sys_dir("not json", "t").unwrap_err(),
            "cargo metadata gave no JSON for t"
        );
        assert!(ort_sys_dir("{}", "t").is_err());
    }
}
