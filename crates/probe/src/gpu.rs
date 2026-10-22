use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Vulkan,
    Metal,
    Dx12,
    Gl,
    BrowserWebGpu,
    Other,
}

impl From<wgpu::Backend> for Backend {
    fn from(value: wgpu::Backend) -> Self {
        match value {
            wgpu::Backend::Vulkan => Backend::Vulkan,
            wgpu::Backend::Metal => Backend::Metal,
            wgpu::Backend::Dx12 => Backend::Dx12,
            wgpu::Backend::Gl => Backend::Gl,
            wgpu::Backend::BrowserWebGpu => Backend::BrowserWebGpu,
            wgpu::Backend::Noop => Backend::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
    Other,
}

impl From<wgpu::DeviceType> for DeviceType {
    fn from(value: wgpu::DeviceType) -> Self {
        match value {
            wgpu::DeviceType::IntegratedGpu => DeviceType::IntegratedGpu,
            wgpu::DeviceType::DiscreteGpu => DeviceType::DiscreteGpu,
            wgpu::DeviceType::VirtualGpu => DeviceType::VirtualGpu,
            wgpu::DeviceType::Cpu => DeviceType::Cpu,
            wgpu::DeviceType::Other => DeviceType::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub name: String,
    pub backend: Backend,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GpuInfo {
    pub adapters: Vec<AdapterInfo>,
}

static GPU: OnceLock<GpuInfo> = OnceLock::new();

static VULKAN_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn serial_vulkan<T>(f: impl FnOnce() -> T) -> T {
    let _serial = VULKAN_GATE.lock().unwrap_or_else(|p| p.into_inner());
    f()
}

pub fn probe_gpu() -> GpuInfo {
    GPU.get_or_init(|| {
        let _serial = VULKAN_GATE.lock().unwrap_or_else(|p| p.into_inner());
        let instance = wgpu::Instance::default();
        let adapters = instance.enumerate_adapters(wgpu::Backends::all());
        let adapters = adapters
            .into_iter()
            .map(|adapter| {
                let info = adapter.get_info();
                AdapterInfo {
                    name: info.name,
                    backend: info.backend.into(),
                    device_type: info.device_type.into(),
                }
            })
            .collect();
        GpuInfo { adapters }
    })
    .clone()
}

static CAN_DRAW_VIDEO: OnceLock<bool> = OnceLock::new();

pub fn can_draw_video() -> bool {
    *CAN_DRAW_VIDEO.get_or_init(|| {
        let _serial = VULKAN_GATE.lock().unwrap_or_else(|p| p.into_inner());
        let instance = wgpu::Instance::default();
        let Some(adapter) =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .ok()
        else {
            return false;
        };
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return false;
        }
        let asked = wgpu::DeviceDescriptor {
            label: Some("PITO MD screen check"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            trace: wgpu::Trace::Off,
        };
        pollster::block_on(adapter.request_device(&asked)).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_never_panics() {
        std::hint::black_box(probe_gpu());
    }

    #[test]
    fn concurrent_gpu_probes_all_observe_the_one_memoized_result() {
        let handles: Vec<_> = (0..32).map(|_| std::thread::spawn(probe_gpu)).collect();
        let results: Vec<GpuInfo> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for result in &results[1..] {
            assert_eq!(
                result, &results[0],
                "every concurrent caller must see the same memoized probe"
            );
        }
    }

    #[test]
    fn concurrent_video_capability_checks_all_observe_the_one_memoized_result() {
        let handles: Vec<_> = (0..32)
            .map(|_| std::thread::spawn(can_draw_video))
            .collect();
        let results: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for result in &results[1..] {
            assert_eq!(
                result, &results[0],
                "every concurrent caller must see the same memoized answer"
            );
        }
    }

    use crate::machine::{Vendor, vendor_of_adapters};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    fn adapter(name: &str, device_type: wgpu::DeviceType) -> AdapterInfo {
        AdapterInfo {
            name: name.to_string(),
            backend: wgpu::Backend::Vulkan.into(),
            device_type: device_type.into(),
        }
    }

    #[test]
    fn every_real_backend_keeps_its_own_name_and_only_noop_is_other() {
        let real = [
            wgpu::Backend::Vulkan,
            wgpu::Backend::Metal,
            wgpu::Backend::Dx12,
            wgpu::Backend::Gl,
            wgpu::Backend::BrowserWebGpu,
        ]
        .map(Backend::from);
        for (i, backend) in real.iter().enumerate() {
            assert_ne!(*backend, Backend::Other, "{backend:?}");
            assert!(!real[i + 1..].contains(backend), "{backend:?} twice");
        }
        assert_eq!(Backend::from(wgpu::Backend::Noop), Backend::Other);
    }

    #[test]
    fn a_software_adapter_never_names_a_vendor_whatever_it_is_called() {
        let adapters = [adapter("NVIDIA GeForce RTX 4090", wgpu::DeviceType::Cpu)];
        assert_eq!(vendor_of_adapters(&adapters), Vendor::None);
    }

    #[test]
    fn only_a_discrete_adapter_outranks_the_others() {
        for other in [
            wgpu::DeviceType::IntegratedGpu,
            wgpu::DeviceType::VirtualGpu,
            wgpu::DeviceType::Other,
        ] {
            let adapters = [
                adapter("Intel(R) Iris Xe", other),
                adapter("AMD Radeon RX 6600", wgpu::DeviceType::DiscreteGpu),
            ];
            assert_eq!(vendor_of_adapters(&adapters), Vendor::Amd, "{other:?}");

            let swapped = [
                adapter("AMD Radeon RX 6600", other),
                adapter("Intel(R) Iris Xe", wgpu::DeviceType::IntegratedGpu),
            ];
            assert_eq!(vendor_of_adapters(&swapped), Vendor::Amd, "{other:?}");
        }
    }

    #[test]
    fn integrated_virtual_and_other_adapters_still_name_a_vendor_alone() {
        for kind in [
            wgpu::DeviceType::IntegratedGpu,
            wgpu::DeviceType::VirtualGpu,
            wgpu::DeviceType::Other,
            wgpu::DeviceType::DiscreteGpu,
        ] {
            let adapters = [
                adapter("llvmpipe (LLVM)", wgpu::DeviceType::Cpu),
                adapter("Intel(R) Arc(TM) A380", kind),
            ];
            assert_eq!(vendor_of_adapters(&adapters), Vendor::Intel, "{kind:?}");
        }
    }

    #[test]
    fn serial_vulkan_never_lets_two_callers_in_at_once() {
        const ROUNDS: usize = 2_000;
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let inside = Arc::clone(&inside);
                let most = Arc::clone(&most);
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    for _ in 0..ROUNDS {
                        serial_vulkan(|| {
                            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                            most.fetch_max(now, Ordering::SeqCst);
                            std::thread::yield_now();
                            inside.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);
        assert_eq!(inside.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn serial_vulkan_hands_back_the_value_and_survives_a_panicking_caller() {
        assert_eq!(serial_vulkan(|| 41 + 1), 42);
        let crashed =
            std::thread::spawn(|| serial_vulkan(|| -> u8 { panic!("caller failed") })).join();
        assert!(crashed.is_err());
        assert_eq!(serial_vulkan(|| "still open"), "still open");
    }

    #[test]
    fn a_card_that_can_draw_video_is_among_the_enumerated_hardware_adapters() {
        if can_draw_video() {
            let adapters = probe_gpu().adapters;
            assert!(
                adapters
                    .iter()
                    .any(|adapter| adapter.device_type != DeviceType::Cpu),
                "{adapters:?}"
            );
        }
    }
}
