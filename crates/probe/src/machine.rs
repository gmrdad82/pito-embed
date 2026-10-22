use crate::accel::{Accel, probe_accel};
use crate::gpu::{AdapterInfo, DeviceType, probe_gpu};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl Os {
    pub const fn label(self) -> &'static str {
        match self {
            Os::Linux => "Linux",
            Os::Windows => "Windows",
            Os::MacOs => "macOS",
            Os::Other => "This computer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    None,
}

impl Vendor {
    pub const fn label(self) -> &'static str {
        match self {
            Vendor::Nvidia => "NVIDIA",
            Vendor::Amd => "AMD",
            Vendor::Intel => "Intel",
            Vendor::Apple => "Apple Silicon",
            Vendor::None => "no graphics card",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Cuda,
    Vulkan,
    Metal,
    DirectMl,
    WebGpu,
    CoreMl,
    VideoToolbox,
    Nvidia,
    Vaapi,
    Qsv,
    Amf,
    Processor,
}

impl Lane {
    pub const fn label(self) -> &'static str {
        match self {
            Lane::Cuda => "CUDA",
            Lane::Vulkan => "Vulkan",
            Lane::Metal => "Metal",
            Lane::DirectMl => "DirectML",
            Lane::WebGpu => "WebGPU",
            Lane::CoreMl => "Core ML",
            Lane::VideoToolbox => "VideoToolbox",
            Lane::Nvidia => "NVDEC and NVENC",
            Lane::Vaapi => "VAAPI",
            Lane::Qsv => "Quick Sync",
            Lane::Amf => "AMF",
            Lane::Processor => "the processor",
        }
    }

    pub const fn accelerated(self) -> bool {
        !matches!(self, Lane::Processor)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub os: Os,
    pub release: Option<String>,
    pub vendor: Vendor,
    pub card: Option<String>,
    pub cores: usize,
    pub ram_bytes: u64,
    pub vram_bytes: Option<u64>,
    pub listening: Lane,
    pub filing: Lane,
    pub video: Lane,
}

impl Machine {
    pub fn accelerated(&self) -> bool {
        self.listening.accelerated() && self.video.accelerated()
    }

    pub fn headline(&self) -> String {
        match self.vendor {
            Vendor::None => format!("{}, processor only", self.os.label()),
            Vendor::Apple => format!("{} on {}", self.os.label(), Vendor::Apple.label()),
            vendor => format!(
                "{} with {} {}",
                self.os.label(),
                vendor.label(),
                self.listening.label()
            ),
        }
    }
}

fn filing_lane(os: Os, vendor: Vendor) -> Lane {
    match (os, vendor) {
        (_, Vendor::None) => Lane::Processor,
        (Os::MacOs, _) => Lane::CoreMl,
        (Os::Windows, _) => Lane::DirectMl,
        (Os::Linux, _) => Lane::WebGpu,
        _ => Lane::Processor,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lanes {
    listening: Lane,
    filing: Lane,
    video: Lane,
}

fn vendor_of_accel(accel: Accel, adapters: impl FnOnce() -> Vec<AdapterInfo>) -> Vendor {
    match accel {
        Accel::NvidiaCuda => Vendor::Nvidia,
        Accel::AppleVideoToolbox => Vendor::Apple,
        Accel::AmdVaapi => Vendor::Amd,
        Accel::IntelQsv => Vendor::Intel,
        Accel::Cpu => vendor_of_adapters(&adapters()),
    }
}

fn lanes(os: Os, vendor: Vendor) -> Lanes {
    let listening = match (os, vendor) {
        (_, Vendor::None) => Lane::Processor,
        (_, Vendor::Nvidia) => Lane::Cuda,
        (Os::MacOs, _) => Lane::Metal,
        _ => Lane::Vulkan,
    };

    let video = match (os, vendor) {
        (_, Vendor::None) => Lane::Processor,
        (_, Vendor::Nvidia) => Lane::Nvidia,
        (Os::MacOs, _) => Lane::VideoToolbox,
        (Os::Windows, Vendor::Amd) => Lane::Amf,
        (Os::Linux, Vendor::Amd) => Lane::Vaapi,
        (_, Vendor::Intel) => Lane::Qsv,
        _ => Lane::Processor,
    };

    Lanes {
        listening,
        filing: filing_lane(os, vendor),
        video,
    }
}

pub fn probe_machine() -> Machine {
    let os = host_os();
    let vendor = vendor_of_accel(probe_accel(), || probe_gpu().adapters);
    let Lanes {
        listening,
        filing,
        video,
    } = lanes(os, vendor);

    Machine {
        os,
        release: os_release(),
        vendor,
        card: card_name(),
        cores: std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1),
        ram_bytes: ram_bytes(),
        vram_bytes: vram_bytes(vendor),
        listening,
        filing,
        video,
    }
}

fn readable_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1_073_741_824.0;
    if gb >= 1.0 {
        format!("{gb:.0} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}

impl Machine {
    pub fn spec_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(card) = &self.card {
            parts.push(short_card(card));
        }
        match (self.vram_bytes, self.vendor) {
            (Some(vram), _) => parts.push(readable_bytes(vram)),
            (None, Vendor::Apple) => parts.push("unified memory".to_string()),
            (None, Vendor::Intel) => parts.push("shared graphics memory".to_string()),
            (None, _) => {}
        }
        parts.push(format!("{} cores", self.cores));
        if self.ram_bytes > 0 {
            parts.push(readable_bytes(self.ram_bytes));
        }
        parts.join(" · ")
    }

    pub fn title(&self) -> String {
        match &self.release {
            Some(release) => format!("{} system information", release),
            None => format!("{} system information", self.os.label()),
        }
    }
}

#[cfg(target_os = "linux")]
fn os_release() -> Option<String> {
    let raw = std::fs::read_to_string("/etc/os-release").ok()?;
    raw.lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim_matches('"').to_string())
}

#[cfg(target_os = "macos")]
fn os_release() -> Option<String> {
    let out = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!version.is_empty()).then(|| format!("macOS {version}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn os_release() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn ram_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|raw| {
            raw.lines()
                .find_map(|line| line.strip_prefix("MemTotal:"))
                .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn ram_bytes() -> u64 {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn ram_bytes() -> u64 {
    0
}

fn vram_bytes(vendor: Vendor) -> Option<u64> {
    match vendor {
        Vendor::Nvidia => nvidia_vram(),
        Vendor::Amd => amd_vram(),
        Vendor::Intel | Vendor::Apple | Vendor::None => None,
    }
}

fn nvidia_vram() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    nvidia_vram_from(&String::from_utf8_lossy(&out.stdout))
}

fn nvidia_vram_from(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .next()?
        .trim()
        .parse::<u64>()
        .ok()?
        .checked_mul(1_048_576)
}

#[cfg(target_os = "linux")]
fn amd_vram() -> Option<u64> {
    amd_vram_from(std::path::Path::new("/sys/class/drm"))
}

#[cfg(not(target_os = "linux"))]
fn amd_vram() -> Option<u64> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn amd_vram_from(drm_root: &std::path::Path) -> Option<u64> {
    let entries = std::fs::read_dir(drm_root).ok()?;
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("card") && !name.contains('-')
        })
        .filter_map(|entry| {
            std::fs::read_to_string(entry.path().join("device/mem_info_vram_total")).ok()
        })
        .filter_map(|raw| raw.trim().parse::<u64>().ok())
        .max()
}

const fn host_os() -> Os {
    if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::MacOs
    } else {
        Os::Other
    }
}

fn vendor_of_name(name: &str) -> Vendor {
    let name = name.to_lowercase();
    if name.contains("nvidia") || name.contains("geforce") || name.contains("rtx") {
        return Vendor::Nvidia;
    }
    if name.contains("amd") || name.contains("radeon") {
        return Vendor::Amd;
    }
    if name.contains("intel") || name.contains("arc") {
        return Vendor::Intel;
    }
    if name.contains("apple") {
        return Vendor::Apple;
    }
    Vendor::None
}

pub(crate) fn vendor_of_adapters(adapters: &[AdapterInfo]) -> Vendor {
    let candidates = adapters
        .iter()
        .filter(|adapter| adapter.device_type != DeviceType::Cpu);
    let discrete = candidates
        .clone()
        .filter(|adapter| adapter.device_type == DeviceType::DiscreteGpu)
        .map(|adapter| vendor_of_name(&adapter.name))
        .find(|vendor| *vendor != Vendor::None);
    if let Some(vendor) = discrete {
        return vendor;
    }
    candidates
        .map(|adapter| vendor_of_name(&adapter.name))
        .find(|vendor| *vendor != Vendor::None)
        .unwrap_or(Vendor::None)
}

const CARD_NOISE: [&str; 12] = [
    "NVIDIA", "GeForce", "AMD", "ATI", "Radeon", "Intel", "Intel(R)", "Apple", "Laptop", "GPU",
    "Graphics", "(TM)",
];

pub fn short_card(name: &str) -> String {
    let kept: Vec<&str> = name
        .split_whitespace()
        .filter(|w| !CARD_NOISE.iter().any(|n| n.eq_ignore_ascii_case(w)))
        .collect();
    if kept.is_empty() {
        name.trim().to_string()
    } else {
        kept.join(" ")
    }
}

fn card_name() -> Option<String> {
    card_of(&probe_gpu().adapters)
}

fn card_of(adapters: &[AdapterInfo]) -> Option<String> {
    adapters
        .iter()
        .find(|adapter| adapter.device_type == DeviceType::DiscreteGpu)
        .or_else(|| {
            adapters
                .iter()
                .find(|adapter| adapter.device_type != DeviceType::Cpu)
        })
        .map(|adapter| adapter.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_gpu_is_processor_regardless_of_os() {
        for os in [Os::Linux, Os::Windows, Os::MacOs, Os::Other] {
            assert_eq!(filing_lane(os, Vendor::None), Lane::Processor);
        }
    }

    #[test]
    fn macos_gets_coreml_regardless_of_vendor() {
        for vendor in [Vendor::Apple, Vendor::Amd, Vendor::Intel, Vendor::Nvidia] {
            assert_eq!(filing_lane(Os::MacOs, vendor), Lane::CoreMl);
        }
    }

    #[test]
    fn windows_gets_directml_regardless_of_vendor() {
        for vendor in [Vendor::Amd, Vendor::Intel, Vendor::Nvidia] {
            assert_eq!(filing_lane(Os::Windows, vendor), Lane::DirectMl);
        }
    }

    #[test]
    fn linux_gets_webgpu_regardless_of_vendor() {
        for vendor in [Vendor::Amd, Vendor::Intel, Vendor::Nvidia] {
            assert_eq!(filing_lane(Os::Linux, vendor), Lane::WebGpu);
        }
    }

    #[test]
    fn an_unrecognized_os_refuses_rather_than_guessing() {
        assert_eq!(filing_lane(Os::Other, Vendor::Nvidia), Lane::Processor);
    }

    fn adapter(name: &str, device_type: crate::gpu::DeviceType) -> crate::gpu::AdapterInfo {
        crate::gpu::AdapterInfo {
            name: name.to_string(),
            backend: crate::gpu::Backend::Vulkan,
            device_type,
        }
    }

    #[test]
    fn the_discrete_adapter_names_the_vendor_even_when_the_igpu_enumerates_first() {
        let adapters = [
            adapter("Intel(R) UHD Graphics 770", DeviceType::IntegratedGpu),
            adapter("NVIDIA GeForce RTX 4080", DeviceType::DiscreteGpu),
        ];
        assert_eq!(vendor_of_adapters(&adapters), Vendor::Nvidia);

        let flipped = [
            adapter("AMD Radeon RX 7900", DeviceType::DiscreteGpu),
            adapter("Intel(R) Iris Xe", DeviceType::IntegratedGpu),
        ];
        assert_eq!(vendor_of_adapters(&flipped), Vendor::Amd);
    }

    #[test]
    fn with_no_discrete_adapter_the_integrated_one_still_names_the_vendor() {
        let adapters = [adapter("Intel(R) Iris Xe", DeviceType::IntegratedGpu)];
        assert_eq!(vendor_of_adapters(&adapters), Vendor::Intel);

        let apple = [adapter("Apple M3 Max", DeviceType::IntegratedGpu)];
        assert_eq!(vendor_of_adapters(&apple), Vendor::Apple);
    }

    #[test]
    fn a_cpu_adapter_alone_is_no_vendor_at_all() {
        let adapters = [adapter("llvmpipe (LLVM)", DeviceType::Cpu)];
        assert_eq!(vendor_of_adapters(&adapters), Vendor::None);
        assert_eq!(vendor_of_adapters(&[]), Vendor::None);
    }

    #[test]
    fn amd_vram_reads_the_biggest_card_from_a_sysfs_shaped_tree() {
        let root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/testscratch");
        std::fs::create_dir_all(&root).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("probe-amd-vram-")
            .tempdir_in(&root)
            .unwrap();
        let drm = dir.path();
        std::fs::create_dir_all(drm.join("card0/device")).unwrap();
        std::fs::create_dir_all(drm.join("card1/device")).unwrap();
        std::fs::create_dir_all(drm.join("card1-DP-1")).unwrap();
        std::fs::write(drm.join("card0/device/mem_info_vram_total"), "536870912\n").unwrap();
        std::fs::write(
            drm.join("card1/device/mem_info_vram_total"),
            "17163091968\n",
        )
        .unwrap();

        assert_eq!(amd_vram_from(drm), Some(17_163_091_968));
    }

    #[test]
    fn amd_vram_on_a_tree_with_no_amdgpu_files_is_none_not_a_panic() {
        let root =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/testscratch");
        std::fs::create_dir_all(&root).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("probe-amd-none-")
            .tempdir_in(&root)
            .unwrap();
        std::fs::create_dir_all(dir.path().join("card0/device")).unwrap();
        assert_eq!(amd_vram_from(dir.path()), None);
        assert_eq!(amd_vram_from(&dir.path().join("missing")), None);
    }

    fn machine_with(vendor: Vendor, vram: Option<u64>) -> Machine {
        Machine {
            os: Os::Linux,
            release: None,
            vendor,
            card: Some("Test Card".to_string()),
            cores: 8,
            ram_bytes: 16 * 1_073_741_824,
            vram_bytes: vram,
            listening: Lane::Vulkan,
            filing: Lane::WebGpu,
            video: Lane::Vaapi,
        }
    }

    #[test]
    fn the_card_reads_short_without_vendor_words() {
        assert_eq!(short_card("NVIDIA GeForce RTX 4080 Laptop GPU"), "RTX 4080");
        assert_eq!(short_card("AMD Radeon RX 7900 XTX"), "RX 7900 XTX");
        assert_eq!(short_card("Apple M2 Max"), "M2 Max");
        assert_eq!(short_card("Intel(R) Iris(R) Xe Graphics"), "Iris(R) Xe");
        assert_eq!(short_card("GPU"), "GPU");
    }

    #[test]
    fn the_spec_line_shows_real_vram_when_a_number_exists() {
        let line = machine_with(Vendor::Amd, Some(16 * 1_073_741_824)).spec_line();
        assert!(line.contains("16 GB"), "{line}");
    }

    #[test]
    fn apple_reads_unified_and_intel_reads_shared_when_no_number_exists() {
        let apple = machine_with(Vendor::Apple, None).spec_line();
        assert!(apple.contains("unified memory"), "{apple}");

        let intel = machine_with(Vendor::Intel, None).spec_line();
        assert!(intel.contains("shared graphics memory"), "{intel}");

        let none = machine_with(Vendor::None, None).spec_line();
        assert!(
            !none.contains("unified") && !none.contains("shared"),
            "{none}"
        );
    }

    const GIB: u64 = 1_073_741_824;
    const MIB: u64 = 1_048_576;

    fn lanes_of(listening: Lane, filing: Lane, video: Lane) -> Lanes {
        Lanes {
            listening,
            filing,
            video,
        }
    }

    #[test]
    fn every_os_and_vendor_that_matters_gets_its_lanes() {
        use Lane::*;
        let table = [
            (Os::Linux, Vendor::Nvidia, lanes_of(Cuda, WebGpu, Nvidia)),
            (Os::Windows, Vendor::Nvidia, lanes_of(Cuda, DirectMl, Nvidia)),
            (Os::Linux, Vendor::Amd, lanes_of(Vulkan, WebGpu, Vaapi)),
            (Os::Windows, Vendor::Amd, lanes_of(Vulkan, DirectMl, Amf)),
            (Os::Linux, Vendor::Intel, lanes_of(Vulkan, WebGpu, Qsv)),
            (Os::Windows, Vendor::Intel, lanes_of(Vulkan, DirectMl, Qsv)),
            (Os::MacOs, Vendor::Apple, lanes_of(Metal, CoreMl, VideoToolbox)),
            (Os::MacOs, Vendor::Amd, lanes_of(Metal, CoreMl, VideoToolbox)),
            (Os::MacOs, Vendor::Intel, lanes_of(Metal, CoreMl, VideoToolbox)),
            (Os::Other, Vendor::Nvidia, lanes_of(Cuda, Processor, Nvidia)),
            (Os::Other, Vendor::Intel, lanes_of(Vulkan, Processor, Qsv)),
        ];
        for (os, vendor, want) in table {
            assert_eq!(lanes(os, vendor), want, "{os:?} {vendor:?}");
        }
    }

    #[test]
    fn no_card_runs_everything_on_the_processor_on_every_os() {
        let all = lanes_of(Lane::Processor, Lane::Processor, Lane::Processor);
        for os in [Os::Linux, Os::Windows, Os::MacOs, Os::Other] {
            assert_eq!(lanes(os, Vendor::None), all, "{os:?}");
        }
    }

    #[test]
    fn quick_sync_is_never_offered_for_a_card_that_is_not_intel() {
        for os in [Os::Linux, Os::Windows, Os::MacOs, Os::Other] {
            for vendor in [Vendor::Nvidia, Vendor::Amd, Vendor::Apple, Vendor::None] {
                assert_ne!(lanes(os, vendor).video, Lane::Qsv, "{os:?} {vendor:?}");
            }
        }
        assert_eq!(lanes(Os::Linux, Vendor::Apple).video, Lane::Processor);
        assert_eq!(lanes(Os::Other, Vendor::Amd).video, Lane::Processor);
    }

    #[test]
    fn a_hardware_accelerator_names_the_vendor_without_enumerating_adapters() {
        let table = [
            (Accel::NvidiaCuda, Vendor::Nvidia),
            (Accel::AppleVideoToolbox, Vendor::Apple),
            (Accel::AmdVaapi, Vendor::Amd),
            (Accel::IntelQsv, Vendor::Intel),
        ];
        for (accel, want) in table {
            let vendor = vendor_of_accel(accel, || panic!("adapters enumerated for {accel:?}"));
            assert_eq!(vendor, want);
        }
    }

    #[test]
    fn with_no_accelerator_the_adapters_name_the_vendor() {
        let nvidia = vendor_of_accel(Accel::Cpu, || {
            vec![adapter("NVIDIA GeForce RTX 3060", DeviceType::DiscreteGpu)]
        });
        assert_eq!(nvidia, Vendor::Nvidia);

        let software = vendor_of_accel(Accel::Cpu, || {
            vec![adapter("llvmpipe (LLVM 19.1.0, 256 bits)", DeviceType::Cpu)]
        });
        assert_eq!(software, Vendor::None);
        assert_eq!(vendor_of_accel(Accel::Cpu, Vec::new), Vendor::None);
    }

    fn machine_on(os: Os, vendor: Vendor) -> Machine {
        let Lanes {
            listening,
            filing,
            video,
        } = lanes(os, vendor);
        Machine {
            os,
            release: None,
            vendor,
            card: None,
            cores: 4,
            ram_bytes: 0,
            vram_bytes: None,
            listening,
            filing,
            video,
        }
    }

    #[test]
    fn a_machine_is_accelerated_only_when_listening_and_video_both_leave_the_processor() {
        assert!(machine_on(Os::Linux, Vendor::Nvidia).accelerated());
        assert!(machine_on(Os::Linux, Vendor::Amd).accelerated());
        assert!(machine_on(Os::Windows, Vendor::Intel).accelerated());
        assert!(machine_on(Os::MacOs, Vendor::Apple).accelerated());
        assert!(!machine_on(Os::Linux, Vendor::None).accelerated());
        assert!(!machine_on(Os::Linux, Vendor::Apple).accelerated());

        let mut video_on_cpu = machine_on(Os::Linux, Vendor::Amd);
        video_on_cpu.video = Lane::Processor;
        assert!(!video_on_cpu.accelerated());

        let mut listening_on_cpu = machine_on(Os::Linux, Vendor::Amd);
        listening_on_cpu.listening = Lane::Processor;
        assert!(!listening_on_cpu.accelerated());
    }

    #[test]
    fn the_headline_reads_for_every_vendor() {
        assert_eq!(
            machine_on(Os::Linux, Vendor::None).headline(),
            "Linux, processor only"
        );
        assert_eq!(
            machine_on(Os::Other, Vendor::None).headline(),
            "This computer, processor only"
        );
        assert_eq!(
            machine_on(Os::MacOs, Vendor::Apple).headline(),
            "macOS on Apple Silicon"
        );
        assert_eq!(
            machine_on(Os::Windows, Vendor::Nvidia).headline(),
            "Windows with NVIDIA CUDA"
        );
        assert_eq!(
            machine_on(Os::Linux, Vendor::Amd).headline(),
            "Linux with AMD Vulkan"
        );
        assert_eq!(
            machine_on(Os::Linux, Vendor::Intel).headline(),
            "Linux with Intel Vulkan"
        );
        assert_eq!(
            machine_on(Os::MacOs, Vendor::Amd).headline(),
            "macOS with AMD Metal"
        );
    }

    #[test]
    fn readable_bytes_switches_to_gigabytes_at_exactly_one() {
        assert_eq!(readable_bytes(512 * MIB), "512 MB");
        assert_eq!(readable_bytes(1023 * MIB), "1023 MB");
        assert_eq!(readable_bytes(0), "0 MB");
        assert_eq!(readable_bytes(GIB), "1 GB");
        assert_eq!(readable_bytes(GIB + GIB / 4), "1 GB");
        assert_eq!(readable_bytes(GIB + 3 * GIB / 4), "2 GB");
        assert_eq!(readable_bytes(24 * GIB), "24 GB");
    }

    #[test]
    fn the_full_spec_line_reads_card_vram_cores_and_ram_in_order() {
        let mut machine = machine_on(Os::Linux, Vendor::Nvidia);
        machine.card = Some("NVIDIA GeForce RTX 4080".to_string());
        machine.vram_bytes = Some(16 * GIB);
        machine.cores = 16;
        machine.ram_bytes = 32 * GIB;
        assert_eq!(machine.spec_line(), "RTX 4080 · 16 GB · 16 cores · 32 GB");
    }

    #[test]
    fn the_spec_line_drops_what_is_unknown() {
        let machine = machine_on(Os::Linux, Vendor::Amd);
        assert_eq!(machine.spec_line(), "4 cores");

        let mut small = machine_on(Os::Linux, Vendor::None);
        small.ram_bytes = 768 * MIB;
        assert_eq!(small.spec_line(), "4 cores · 768 MB");
    }

    #[test]
    fn the_title_prefers_the_release_name_and_falls_back_to_the_os() {
        let mut machine = machine_on(Os::Linux, Vendor::Amd);
        assert_eq!(machine.title(), "Linux system information");
        machine.release = Some("Arch Linux".to_string());
        assert_eq!(machine.title(), "Arch Linux system information");
        assert_eq!(
            machine_on(Os::Other, Vendor::None).title(),
            "This computer system information"
        );
    }

    #[test]
    fn vram_is_never_guessed_for_cards_that_share_memory_or_no_card() {
        for vendor in [Vendor::Intel, Vendor::Apple, Vendor::None] {
            assert_eq!(vram_bytes(vendor), None, "{vendor:?}");
        }
    }

    #[test]
    fn nvidia_smi_output_reads_the_first_card_in_bytes() {
        assert_eq!(nvidia_vram_from("16376\n"), Some(16376 * MIB));
        assert_eq!(nvidia_vram_from("  8192  \n"), Some(8192 * MIB));
        assert_eq!(nvidia_vram_from("24564\n12288\n"), Some(24564 * MIB));
    }

    #[test]
    fn nvidia_smi_output_that_is_empty_or_garbage_is_no_number() {
        assert_eq!(nvidia_vram_from(""), None);
        assert_eq!(nvidia_vram_from("\n16376\n"), None);
        assert_eq!(nvidia_vram_from("[N/A]\n"), None);
        assert_eq!(
            nvidia_vram_from(
                "NVIDIA-SMI has failed because it couldn't communicate with the NVIDIA driver.\n"
            ),
            None
        );
        assert_eq!(nvidia_vram_from("-1\n"), None);
        assert_eq!(nvidia_vram_from("16376 MiB\n"), None);
    }

    #[test]
    fn nvidia_smi_output_too_big_to_be_bytes_is_no_number_not_an_overflow() {
        assert_eq!(nvidia_vram_from(&format!("{}\n", u64::MAX / MIB + 1)), None);
        assert_eq!(
            nvidia_vram_from(&format!("{}\n", u64::MAX / MIB)),
            Some(u64::MAX / MIB * MIB)
        );
    }

    #[test]
    fn every_vendor_word_names_its_vendor() {
        let table = [
            ("NVIDIA Quadro P2000", Vendor::Nvidia),
            ("GeForce GTX 1080", Vendor::Nvidia),
            ("RTX A4000", Vendor::Nvidia),
            ("AMD Radeon RX 7900 XTX", Vendor::Amd),
            ("Radeon Pro W6800", Vendor::Amd),
            ("Intel(R) UHD Graphics 630", Vendor::Intel),
            ("Arc A770", Vendor::Intel),
            ("Apple M3 Max", Vendor::Apple),
        ];
        for (name, want) in table {
            assert_eq!(vendor_of_name(name), want, "{name}");
        }
    }

    #[test]
    fn a_name_with_no_vendor_word_is_no_vendor() {
        for name in [
            "llvmpipe (LLVM 19.1.0, 256 bits)",
            "Microsoft Basic Render Driver",
            "SwiftShader Device (Subzero)",
            "",
        ] {
            assert_eq!(vendor_of_name(name), Vendor::None, "{name}");
        }
    }

    #[test]
    fn short_card_keeps_the_model_and_trims_a_name_of_only_vendor_words() {
        assert_eq!(short_card("Intel(R) Arc(TM) A770 Graphics"), "Arc(TM) A770");
        assert_eq!(short_card("  NVIDIA   GPU  "), "NVIDIA   GPU");
        assert_eq!(short_card("nvidia geforce rtx 3060"), "rtx 3060");
        assert_eq!(short_card(""), "");
    }

    #[test]
    fn the_card_name_prefers_the_discrete_adapter_then_any_real_one() {
        let both = [
            adapter("Intel(R) UHD Graphics 770", DeviceType::IntegratedGpu),
            adapter("AMD Radeon RX 7800 XT", DeviceType::DiscreteGpu),
        ];
        assert_eq!(card_of(&both).as_deref(), Some("AMD Radeon RX 7800 XT"));

        let fallback = [
            adapter("llvmpipe (LLVM)", DeviceType::Cpu),
            adapter("Virtio GPU", DeviceType::VirtualGpu),
        ];
        assert_eq!(card_of(&fallback).as_deref(), Some("Virtio GPU"));

        let software = [adapter("llvmpipe (LLVM)", DeviceType::Cpu)];
        assert_eq!(card_of(&software), None);
        assert_eq!(card_of(&[]), None);
    }
}
