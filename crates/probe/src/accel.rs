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
    use std::path::Path;

    pub fn probe() -> Accel {
        if nvidia_present() {
            return Accel::NvidiaCuda;
        }
        pick(&render_node_vendors())
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

    fn nvidia_present() -> bool {
        Path::new("/dev/nvidia0").exists() || nvidia_smi_on_path()
    }

    fn nvidia_smi_on_path() -> bool {
        let Some(path_var) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&path_var).any(|dir| dir.join("nvidia-smi").is_file())
    }

    fn render_node_vendors() -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir("/dev/dri") else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("renderD"))
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|name| {
                let contents =
                    std::fs::read_to_string(format!("/sys/class/drm/{name}/device/vendor")).ok()?;
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
}
