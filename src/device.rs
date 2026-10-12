use crate::error::EmbedError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedDevice {
    WebGpu,
    DirectMl,
    CoreMl,
    Cpu,
    #[cfg(debug_assertions)]
    Broken,
}

impl EmbedDevice {
    pub const fn compiled() -> Self {
        if cfg!(target_os = "linux") {
            EmbedDevice::WebGpu
        } else if cfg!(target_os = "windows") {
            EmbedDevice::DirectMl
        } else {
            EmbedDevice::CoreMl
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            EmbedDevice::WebGpu => "Vulkan",
            EmbedDevice::DirectMl => "DirectML",
            EmbedDevice::CoreMl => "CoreML",
            EmbedDevice::Cpu => "the processor",
            #[cfg(debug_assertions)]
            EmbedDevice::Broken => "a broken card (test door)",
        }
    }
}

impl TryFrom<probe::Lane> for EmbedDevice {
    type Error = EmbedError;

    fn try_from(lane: probe::Lane) -> Result<Self, Self::Error> {
        match lane {
            probe::Lane::WebGpu => Ok(EmbedDevice::WebGpu),
            probe::Lane::DirectMl => Ok(EmbedDevice::DirectMl),
            probe::Lane::CoreMl => Ok(EmbedDevice::CoreMl),
            probe::Lane::Processor => Ok(EmbedDevice::Cpu),
            lane => Err(EmbedError::GpuUnavailable {
                detail: format!("{lane:?} is not an embedding execution provider"),
            }),
        }
    }
}

pub(crate) fn provider(device: EmbedDevice) -> ort::ep::ExecutionProviderDispatch {
    match device {
        #[cfg(target_os = "linux")]
        EmbedDevice::WebGpu => ort::ep::WebGPU::default().build().error_on_failure(),
        #[cfg(target_os = "windows")]
        EmbedDevice::DirectMl => ort::ep::DirectML::default().build().error_on_failure(),
        #[cfg(target_os = "macos")]
        EmbedDevice::CoreMl => ort::ep::CoreML::default().build().error_on_failure(),
        EmbedDevice::Cpu => ort::ep::CPU::default().build().error_on_failure(),
        #[allow(unreachable_patterns)]
        _ => unreachable!("a platform GPU provider cannot be used on another platform"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_ml_lane_to_its_execution_provider() {
        assert_eq!(
            EmbedDevice::try_from(probe::Lane::WebGpu),
            Ok(EmbedDevice::WebGpu)
        );
        assert_eq!(
            EmbedDevice::try_from(probe::Lane::DirectMl),
            Ok(EmbedDevice::DirectMl)
        );
        assert_eq!(
            EmbedDevice::try_from(probe::Lane::CoreMl),
            Ok(EmbedDevice::CoreMl)
        );
        assert_eq!(
            EmbedDevice::try_from(probe::Lane::Processor),
            Ok(EmbedDevice::Cpu)
        );
    }

    #[test]
    fn labels_name_the_engine_the_build_drives() {
        assert_eq!(EmbedDevice::WebGpu.label(), "Vulkan");
        assert_eq!(EmbedDevice::DirectMl.label(), "DirectML");
        assert_eq!(EmbedDevice::CoreMl.label(), "CoreML");
        assert_eq!(EmbedDevice::Cpu.label(), "the processor");
    }
}
