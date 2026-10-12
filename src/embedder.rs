use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use fastembed::{
    InitOptionsUserDefined, OutputKey, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};

use crate::device::{self, EmbedDevice};
use crate::error::EmbedError;

const MAX_LENGTH: usize = 128;
const QUERY_PREFIX: &str = "task: search result | query: ";
const DOC_PREFIX: &str = "title: none | text: ";
const ONNX_FILE: &str = "model_q4.onnx";
const ONNX_DATA_FILE: &str = "model_q4.onnx_data";
const DETAIL_CAP_BYTES: usize = 4096;
static WEBGPU_SESSION: Mutex<()> = Mutex::new(());

pub struct Embedder {
    inner: Option<TextEmbedding>,
    device: EmbedDevice,
    fall_off_reason: Option<String>,
}

impl Embedder {
    pub fn load(model_dir: impl AsRef<Path>, device: EmbedDevice) -> Result<Self, EmbedError> {
        let model_dir = model_dir.as_ref();
        match Self::load_on(model_dir, device) {
            Err(EmbedError::GpuUnavailable { detail }) if device != EmbedDevice::Cpu => {
                let mut fallen = Self::load_on(model_dir, EmbedDevice::Cpu)?;
                fallen.fall_off_reason = Some(detail);
                Ok(fallen)
            }
            outcome => outcome,
        }
    }

    pub fn device_in_use(&self) -> EmbedDevice {
        self.device
    }

    pub fn fall_off_reason(&self) -> Option<&str> {
        self.fall_off_reason.as_deref()
    }

    fn load_on(model_dir: &Path, device: EmbedDevice) -> Result<Self, EmbedError> {
        let _webgpu = webgpu_session(device);
        let onnx_file = read_required(&model_dir.join("onnx").join(ONNX_FILE))?;
        let onnx_data = read_required(&model_dir.join("onnx").join(ONNX_DATA_FILE))?;
        let tokenizer_file = read_required(&model_dir.join("tokenizer.json"))?;
        let config_file = read_required(&model_dir.join("config.json"))?;
        let special_tokens_map_file = read_required(&model_dir.join("special_tokens_map.json"))?;
        let tokenizer_config_file = read_required(&model_dir.join("tokenizer_config.json"))?;

        ensure_provider_available(device)?;

        let mut model = UserDefinedEmbeddingModel::new(
            onnx_file,
            TokenizerFiles {
                tokenizer_file,
                config_file,
                special_tokens_map_file,
                tokenizer_config_file,
            },
        )
        .with_pooling(Pooling::Mean)
        .with_quantization(QuantizationMode::None)
        .with_external_initializer(ONNX_DATA_FILE.to_string(), onnx_data);
        model.output_key = Some(OutputKey::ByName("sentence_embedding"));

        let execution_providers = providers(device)?;
        let options = InitOptionsUserDefined::new()
            .with_execution_providers(execution_providers)
            .with_max_length(MAX_LENGTH);

        let inner = TextEmbedding::try_new_from_user_defined(model, options).map_err(|e| {
            EmbedError::LoadFailed {
                detail: tail_capped(e.to_string()),
            }
        })?;

        Ok(Self {
            inner: Some(inner),
            device,
            fall_off_reason: None,
        })
    }

    pub fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let _webgpu = webgpu_session(self.device);
        let mut out = self
            .inner
            .as_mut()
            .expect("embedder session exists until drop")
            .embed(vec![prefixed_query(text)], None)
            .map_err(inference_err)?;
        let vec = out.pop().ok_or_else(|| EmbedError::Inference {
            detail: "embed_query produced no output".to_string(),
        })?;
        Ok(normalize(&vec))
    }

    pub fn embed_documents(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let _webgpu = webgpu_session(self.device);
        let prefixed: Vec<String> = texts.iter().map(|t| prefixed_document(t)).collect();
        let out = self
            .inner
            .as_mut()
            .expect("embedder session exists until drop")
            .embed(prefixed, None)
            .map_err(inference_err)?;
        Ok(out.iter().map(|v| normalize(v)).collect())
    }
}

impl Drop for Embedder {
    fn drop(&mut self) {
        let _webgpu = webgpu_session(self.device);
        drop(self.inner.take());
    }
}

fn webgpu_session(device: EmbedDevice) -> Option<MutexGuard<'static, ()>> {
    (device == EmbedDevice::WebGpu).then(|| {
        WEBGPU_SESSION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    })
}

fn prefixed_query(text: &str) -> String {
    format!("{QUERY_PREFIX}{text}")
}

fn prefixed_document(text: &str) -> String {
    format!("{DOC_PREFIX}{text}")
}

fn read_required(path: &Path) -> Result<Vec<u8>, EmbedError> {
    std::fs::read(path).map_err(|_| EmbedError::ModelMissing {
        path: path.to_path_buf(),
    })
}

fn providers(device: EmbedDevice) -> Result<Vec<ort::ep::ExecutionProviderDispatch>, EmbedError> {
    let compiled = EmbedDevice::compiled();
    #[cfg(debug_assertions)]
    if device == EmbedDevice::Broken {
        return Err(EmbedError::GpuUnavailable {
            detail: "the test door: this card's provider always fails to build".to_string(),
        });
    }
    if device != EmbedDevice::Cpu && device != compiled {
        return Err(EmbedError::GpuUnavailable {
            detail: format!(
                "{} requested but this build carries {}",
                device.label(),
                compiled.label()
            ),
        });
    }
    Ok(vec![device::provider(device)])
}

fn ensure_provider_available(device: EmbedDevice) -> Result<(), EmbedError> {
    let builder = ort::session::Session::builder().map_err(|e| EmbedError::GpuUnavailable {
        detail: e.to_string(),
    })?;
    builder
        .with_execution_providers(providers(device)?)
        .map_err(|e| EmbedError::GpuUnavailable {
            detail: e.to_string(),
        })?;
    Ok(())
}

fn tail_capped(detail: String) -> String {
    if detail.len() <= DETAIL_CAP_BYTES {
        return detail;
    }
    let mut start = detail.len() - DETAIL_CAP_BYTES;
    while start < detail.len() && !detail.is_char_boundary(start) {
        start += 1;
    }
    detail[start..].to_string()
}

fn inference_err(e: fastembed::Error) -> EmbedError {
    EmbedError::Inference {
        detail: e.to_string(),
    }
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter().map(|x| x / n).collect()
    } else {
        v.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_prefix_is_baked_in_verbatim() {
        assert_eq!(
            prefixed_query("boss fight"),
            "task: search result | query: boss fight"
        );
    }

    #[test]
    fn document_prefix_is_baked_in_verbatim() {
        assert_eq!(
            prefixed_document("boss fight"),
            "title: none | text: boss fight"
        );
    }

    #[test]
    fn query_and_document_prefixes_diverge() {
        assert_ne!(prefixed_query("same text"), prefixed_document("same text"));
    }

    #[test]
    fn normalize_produces_unit_vectors() {
        let v = vec![3.0_f32, 4.0];
        let n = normalize(&v);
        let mag = n.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((mag - 1.0).abs() < 1e-6);
        assert_eq!(n, vec![0.6, 0.8]);
    }

    #[test]
    fn normalize_zero_vector_is_safe() {
        let v = vec![0.0_f32; 768];
        let n = normalize(&v);
        assert_eq!(n, v);
        assert!(n.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn tail_capped_leaves_short_strings_untouched() {
        let short = "backend refused: bad graph".to_string();
        assert_eq!(tail_capped(short.clone()), short);
    }

    #[test]
    fn tail_capped_bounds_long_strings_to_the_cap() {
        let long = "x".repeat(DETAIL_CAP_BYTES * 3);
        let capped = tail_capped(long);
        assert_eq!(capped.len(), DETAIL_CAP_BYTES);
    }

    #[test]
    fn tail_capped_keeps_the_tail_not_the_head() {
        let long = format!("{}TAIL-MARKER", "head".repeat(DETAIL_CAP_BYTES * 2));
        let capped = tail_capped(long.clone());
        assert!(capped.ends_with("TAIL-MARKER"));
        assert_eq!(capped, long[long.len() - DETAIL_CAP_BYTES..]);
    }

    #[test]
    fn tail_capped_respects_utf8_boundaries() {
        let mut long = "a".repeat(DETAIL_CAP_BYTES - 1);
        long.push('中');
        long.push_str(&"b".repeat(10));
        let capped = tail_capped(long);
        assert!(std::str::from_utf8(capped.as_bytes()).is_ok());
        assert!(capped.len() <= DETAIL_CAP_BYTES);
    }
}
