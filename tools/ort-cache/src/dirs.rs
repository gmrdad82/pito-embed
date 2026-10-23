use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Os {
    Linux,
    Macos,
    Other(String),
}

impl Os {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Os::Linux,
            "macos" => Os::Macos,
            other => Os::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CacheEnv {
    pub ort_cache_dir: Option<OsString>,
    pub xdg_cache_home: Option<OsString>,
    pub home: Option<PathBuf>,
}

pub fn cache_root(os: &Os, env: &CacheEnv) -> Result<PathBuf, String> {
    if let Some(dir) = &env.ort_cache_dir {
        let dir = PathBuf::from(dir);
        if !dir.is_absolute() {
            return Err("ORT_CACHE_DIR must be an absolute path".into());
        }
        return Ok(dir);
    }
    let home = || {
        env.home
            .clone()
            .filter(|h| !h.as_os_str().is_empty())
            .ok_or_else(|| "no home folder to hold the cache".to_string())
    };
    match os {
        Os::Linux => match env.xdg_cache_home.as_ref().map(PathBuf::from) {
            Some(xdg) if xdg.is_absolute() => Ok(xdg),
            _ => Ok(home()?.join(".cache/ort.pyke.io")),
        },
        Os::Macos => Ok(home()?.join("Library/Caches/ort.pyke.io")),
        Os::Other(name) => Err(format!(
            "ort-sys keeps no cache folder this tool knows on {name}"
        )),
    }
}

pub fn dest(root: &Path, target: &str, sha256: &str) -> PathBuf {
    root.join("dfbin").join(target).join(sha256)
}

pub fn library(target: &str) -> &'static str {
    if target.contains("windows") {
        "onnxruntime.lib"
    } else {
        "libonnxruntime.a"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(ort: Option<&str>, xdg: Option<&str>, home: Option<&str>) -> CacheEnv {
        CacheEnv {
            ort_cache_dir: ort.map(OsString::from),
            xdg_cache_home: xdg.map(OsString::from),
            home: home.map(PathBuf::from),
        }
    }

    fn root(
        os: &Os,
        ort: Option<&str>,
        xdg: Option<&str>,
        home: Option<&str>,
    ) -> Result<PathBuf, String> {
        cache_root(os, &env(ort, xdg, home))
    }

    #[test]
    fn linux_uses_the_home_cache_unless_xdg_is_absolute() {
        let h = Some("/h");
        assert_eq!(
            root(&Os::Linux, None, None, h).unwrap(),
            Path::new("/h/.cache/ort.pyke.io")
        );
        assert_eq!(
            root(&Os::Linux, None, Some("/x"), h).unwrap(),
            Path::new("/x")
        );
        assert_eq!(
            root(&Os::Linux, None, Some("relative/cache"), h).unwrap(),
            Path::new("/h/.cache/ort.pyke.io")
        );
        assert_eq!(
            root(&Os::Linux, None, Some(""), h).unwrap(),
            Path::new("/h/.cache/ort.pyke.io")
        );
        assert_eq!(
            root(&Os::Linux, None, Some("/x"), None).unwrap(),
            Path::new("/x")
        );
    }

    #[test]
    fn macos_ignores_xdg_and_uses_library_caches() {
        assert_eq!(
            root(&Os::Macos, None, Some("/x"), Some("/h")).unwrap(),
            Path::new("/h/Library/Caches/ort.pyke.io")
        );
    }

    #[test]
    fn an_absolute_ort_cache_dir_wins_on_every_os() {
        for os in [Os::Linux, Os::Macos, Os::Other("freebsd".into())] {
            assert_eq!(
                root(&os, Some("/o"), Some("/x"), None).unwrap(),
                Path::new("/o"),
                "{os:?}"
            );
        }
    }

    #[test]
    fn a_relative_or_empty_ort_cache_dir_is_refused() {
        for bad in ["", "relative/ort"] {
            assert_eq!(
                root(&Os::Linux, Some(bad), None, Some("/h")).unwrap_err(),
                "ORT_CACHE_DIR must be an absolute path"
            );
        }
    }

    #[test]
    fn no_home_is_refused_where_the_home_is_needed() {
        for home in [None, Some("")] {
            assert_eq!(
                root(&Os::Linux, None, None, home).unwrap_err(),
                "no home folder to hold the cache"
            );
            assert_eq!(
                root(&Os::Macos, None, None, home).unwrap_err(),
                "no home folder to hold the cache"
            );
        }
    }

    #[test]
    fn another_os_has_no_known_cache_folder() {
        assert_eq!(
            root(&Os::Other("freebsd".into()), None, None, Some("/h")).unwrap_err(),
            "ort-sys keeps no cache folder this tool knows on freebsd"
        );
    }

    #[test]
    fn the_folder_and_library_follow_the_target() {
        assert_eq!(dest(Path::new("/c"), "t", "ab"), Path::new("/c/dfbin/t/ab"));
        assert_eq!(library("x86_64-pc-windows-msvc"), "onnxruntime.lib");
        assert_eq!(library("aarch64-apple-darwin"), "libonnxruntime.a");
    }

    #[test]
    fn the_current_os_is_linux_or_macos_here() {
        assert!(matches!(Os::current(), Os::Linux | Os::Macos));
    }
}
