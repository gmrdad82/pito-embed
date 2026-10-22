#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accel {
    NvidiaCuda,
    AppleVideoToolbox,
    AmdVaapi,
    IntelQsv,
    Cpu,
}

#[cfg(target_os = "linux")]
pub fn probe_accel() -> Accel {
    linux::probe()
}

#[cfg(target_os = "macos")]
pub fn probe_accel() -> Accel {
    Accel::AppleVideoToolbox
}

#[cfg(target_os = "windows")]
pub fn probe_accel() -> Accel {
    windows::probe()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn probe_accel() -> Accel {
    Accel::Cpu
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Accel;
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub fn probe() -> Accel {
        probe_at(Path::new("/"), std::env::var_os("PATH").as_deref())
    }

    pub(super) fn probe_at(root: &Path, path_var: Option<&OsStr>) -> Accel {
        if nvidia_present(root, path_var) {
            return Accel::NvidiaCuda;
        }
        pick(&render_node_vendors(root))
    }

    pub(super) fn pick(vendors: &[u32]) -> Accel {
        if vendors.contains(&0x1002) {
            return Accel::AmdVaapi;
        }
        if vendors.contains(&0x8086) {
            return Accel::IntelQsv;
        }
        Accel::Cpu
    }

    fn nvidia_present(root: &Path, path_var: Option<&OsStr>) -> bool {
        root.join("dev/nvidia0").exists() || nvidia_smi_on_path(path_var)
    }

    pub(super) fn nvidia_smi_on_path(path_var: Option<&OsStr>) -> bool {
        let Some(path_var) = path_var else {
            return false;
        };
        std::env::split_paths(path_var).any(|dir| executable(&dir.join("nvidia-smi")))
    }

    fn executable(path: &Path) -> bool {
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }

    pub(super) fn render_node_vendors(root: &Path) -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir(root.join("dev/dri")) else {
            return Vec::new();
        };
        let drm = root.join("sys/class/drm");
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("renderD"))
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|name| {
                let contents = std::fs::read_to_string(drm.join(name).join("device/vendor")).ok()?;
                u32::from_str_radix(contents.trim().trim_start_matches("0x"), 16).ok()
            })
            .collect()
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::Accel;

    pub fn probe() -> Accel {
        if nvidia_smi_on_path() {
            Accel::NvidiaCuda
        } else {
            Accel::Cpu
        }
    }

    fn nvidia_smi_on_path() -> bool {
        let Some(path_var) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&path_var).any(|dir| dir.join("nvidia-smi.exe").is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_accel_never_panics() {
        std::hint::black_box(probe_accel());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_discrete_amd_wins_over_an_intel_igpu_whichever_node_comes_first() {
        assert_eq!(linux::pick(&[0x8086, 0x1002]), Accel::AmdVaapi);
        assert_eq!(linux::pick(&[0x1002, 0x8086]), Accel::AmdVaapi);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_lone_intel_node_is_quicksync_and_nothing_is_cpu() {
        assert_eq!(linux::pick(&[0x8086]), Accel::IntelQsv);
        assert_eq!(linux::pick(&[]), Accel::Cpu);
        assert_eq!(linux::pick(&[0xffff]), Accel::Cpu);
    }

    #[cfg(target_os = "linux")]
    mod sysfs {
        use super::super::linux::{nvidia_smi_on_path, probe_at, render_node_vendors};
        use super::super::Accel;
        use std::ffi::{OsStr, OsString};
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        fn node(root: &Path, name: &str, vendor: Option<&str>) {
            std::fs::create_dir_all(root.join("dev/dri")).unwrap();
            std::fs::write(root.join("dev/dri").join(name), "").unwrap();
            if let Some(vendor) = vendor {
                let device = root.join("sys/class/drm").join(name).join("device");
                std::fs::create_dir_all(&device).unwrap();
                std::fs::write(device.join("vendor"), vendor).unwrap();
            }
        }

        fn tool(dir: &Path, name: &str, mode: u32) {
            std::fs::create_dir_all(dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        fn path_of(dirs: &[&PathBuf]) -> Option<OsString> {
            Some(std::env::join_paths(dirs).unwrap())
        }

        #[test]
        fn a_tree_with_no_render_nodes_has_no_vendors() {
            let dir = crate::testscratch::dir("accel-empty");
            assert!(render_node_vendors(dir.path()).is_empty());
            std::fs::create_dir_all(dir.path().join("dev/dri")).unwrap();
            assert!(render_node_vendors(dir.path()).is_empty());
            assert_eq!(probe_at(dir.path(), None), Accel::Cpu);
        }

        #[test]
        fn vendor_ids_read_with_and_without_the_hex_prefix() {
            let dir = crate::testscratch::dir("accel-hex");
            node(dir.path(), "renderD128", Some("0x1002\n"));
            node(dir.path(), "renderD129", Some("8086"));
            assert_eq!(render_node_vendors(dir.path()), vec![0x1002, 0x8086]);
        }

        #[test]
        fn several_cards_read_in_node_order_and_only_render_nodes_count() {
            let dir = crate::testscratch::dir("accel-several");
            node(dir.path(), "renderD130", Some("0x10de\n"));
            node(dir.path(), "renderD128", Some("0x8086\n"));
            node(dir.path(), "renderD129", Some("0x1002\n"));
            node(dir.path(), "card0", Some("0x1002\n"));
            assert_eq!(
                render_node_vendors(dir.path()),
                vec![0x8086, 0x1002, 0x10de]
            );
            assert_eq!(probe_at(dir.path(), None), Accel::AmdVaapi);
        }

        #[test]
        fn an_unreadable_or_garbled_vendor_file_skips_that_node_only() {
            let dir = crate::testscratch::dir("accel-unreadable");
            node(dir.path(), "renderD128", None);
            std::fs::create_dir_all(dir.path().join("sys/class/drm/renderD128/device/vendor"))
                .unwrap();
            node(dir.path(), "renderD129", Some("not hex\n"));
            node(dir.path(), "renderD130", None);
            node(dir.path(), "renderD131", Some("0x8086\n"));
            assert_eq!(render_node_vendors(dir.path()), vec![0x8086]);
            assert_eq!(probe_at(dir.path(), None), Accel::IntelQsv);
        }

        #[test]
        fn an_nvidia_device_node_means_cuda_whatever_the_render_nodes_say() {
            let dir = crate::testscratch::dir("accel-nvidia-node");
            node(dir.path(), "renderD128", Some("0x1002\n"));
            std::fs::write(dir.path().join("dev/nvidia0"), "").unwrap();
            assert_eq!(probe_at(dir.path(), None), Accel::NvidiaCuda);
        }

        #[test]
        fn nvidia_smi_counts_only_when_an_executable_sits_on_the_path() {
            let dir = crate::testscratch::dir("accel-path");
            let empty = dir.path().join("empty");
            let present = dir.path().join("present");
            let plain = dir.path().join("plain");
            let folder = dir.path().join("folder");
            std::fs::create_dir_all(&empty).unwrap();
            tool(&present, "nvidia-smi", 0o755);
            tool(&plain, "nvidia-smi", 0o644);
            std::fs::create_dir_all(folder.join("nvidia-smi")).unwrap();

            assert!(nvidia_smi_on_path(path_of(&[&empty, &present]).as_deref()));
            assert!(!nvidia_smi_on_path(path_of(&[&empty]).as_deref()));
            assert!(!nvidia_smi_on_path(path_of(&[&plain]).as_deref()));
            assert!(!nvidia_smi_on_path(path_of(&[&folder]).as_deref()));
            assert!(!nvidia_smi_on_path(path_of(&[&dir.path().join("missing")]).as_deref()));
            assert!(!nvidia_smi_on_path(Some(OsStr::new(""))));
            assert!(!nvidia_smi_on_path(None));
        }

        #[test]
        fn nvidia_smi_on_the_path_means_cuda_on_a_tree_with_no_device_node() {
            let dir = crate::testscratch::dir("accel-path-probe");
            let bin = dir.path().join("bin");
            tool(&bin, "nvidia-smi", 0o700);
            node(dir.path(), "renderD128", Some("0x8086\n"));
            assert_eq!(
                probe_at(dir.path(), path_of(&[&bin]).as_deref()),
                Accel::NvidiaCuda
            );
            let plain = dir.path().join("plain");
            tool(&plain, "nvidia-smi", 0o600);
            assert_eq!(
                probe_at(dir.path(), path_of(&[&plain]).as_deref()),
                Accel::IntelQsv
            );
        }
    }
}
