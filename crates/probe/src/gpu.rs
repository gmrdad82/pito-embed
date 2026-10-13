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
}
