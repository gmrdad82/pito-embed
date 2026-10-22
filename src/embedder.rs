use std::path::{Path, PathBuf};
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
        let [
            onnx_path,
            onnx_data_path,
            tokenizer_path,
            config_path,
            special_tokens_map_path,
            tokenizer_config_path,
        ] = required_paths(model_dir);
        for path in required_paths(model_dir) {
            check_required(&path)?;
        }

        ensure_provider_available(device)?;

        let onnx_file = read_required(&onnx_path)?;
        let onnx_data = read_required(&onnx_data_path)?;
        let tokenizer_file = read_required(&tokenizer_path)?;
        let config_file = read_required(&config_path)?;
        let special_tokens_map_file = read_required(&special_tokens_map_path)?;
        let tokenizer_config_file = read_required(&tokenizer_config_path)?;

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

fn required_paths(model_dir: &Path) -> [PathBuf; 6] {
    let onnx = model_dir.join("onnx");
    [
        onnx.join(ONNX_FILE),
        onnx.join(ONNX_DATA_FILE),
        model_dir.join("tokenizer.json"),
        model_dir.join("config.json"),
        model_dir.join("special_tokens_map.json"),
        model_dir.join("tokenizer_config.json"),
    ]
}

fn check_required(path: &Path) -> Result<(), EmbedError> {
    let meta = std::fs::metadata(path).map_err(|e| required_err(path, e))?;
    if meta.is_file() {
        Ok(())
    } else {
        Err(EmbedError::LoadFailed {
            detail: format!("{} is not a file", path.display()),
        })
    }
}

fn read_required(path: &Path) -> Result<Vec<u8>, EmbedError> {
    std::fs::read(path).map_err(|e| required_err(path, e))
}

fn required_err(path: &Path, e: std::io::Error) -> EmbedError {
    if e.kind() == std::io::ErrorKind::NotFound {
        EmbedError::ModelMissing {
            path: path.to_path_buf(),
        }
    } else {
        EmbedError::LoadFailed {
            detail: tail_capped(format!("{}: {e}", path.display())),
        }
    }
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
    if v.iter().any(|x| !x.is_finite()) {
        return v.to_vec();
    }
    let scale = v.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    if scale <= 0.0 {
        return v.to_vec();
    }
    let n = v.iter().map(|x| (x / scale).powi(2)).sum::<f32>().sqrt();
    let k = 1.0 / n;
    v.iter().map(|x| x / scale * k).collect()
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

    fn assert_unit_and_parallel(input: &[f32], out: &[f32]) {
        assert_eq!(out.len(), input.len());
        let mag = out
            .iter()
            .map(|x| f64::from(*x).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!((mag - 1.0).abs() < 1e-5, "magnitude {mag} for {input:?}");
        for (x, y) in input.iter().zip(out) {
            assert!(y.is_finite(), "{out:?}");
            assert_eq!(x.signum(), y.signum(), "{input:?} -> {out:?}");
        }
        let in_mag = input
            .iter()
            .map(|x| f64::from(*x).powi(2))
            .sum::<f64>()
            .sqrt();
        for (x, y) in input.iter().zip(out) {
            assert!((f64::from(*x) / in_mag - f64::from(*y)).abs() < 1e-5);
        }
    }

    #[test]
    fn normalize_keeps_the_direction_of_negative_vectors() {
        let v = [-3.0_f32, -4.0];
        let n = normalize(&v);
        assert_eq!(n, vec![-0.6, -0.8]);
        assert_unit_and_parallel(&v, &n);
        let mixed = [-1.0_f32, 2.0, -2.0];
        assert_unit_and_parallel(&mixed, &normalize(&mixed));
    }

    #[test]
    fn normalize_scales_tiny_vectors_whose_squares_underflow_up_to_unit_length() {
        let v = [1e-30_f32, -1e-30, 2e-30];
        assert_eq!(v[0] * v[0], 0.0);
        assert_unit_and_parallel(&v, &normalize(&v));
    }

    #[test]
    fn normalize_scales_huge_vectors_whose_squares_overflow_down_to_unit_length() {
        let v = [3e30_f32, -4e30];
        assert!((v[0] * v[0]).is_infinite());
        assert_unit_and_parallel(&v, &normalize(&v));
    }

    #[test]
    fn normalize_reaches_unit_length_at_the_ends_of_the_f32_range() {
        let largest = [f32::MAX, -f32::MAX, f32::MAX];
        assert_unit_and_parallel(&largest, &normalize(&largest));
        let smallest = [f32::from_bits(1), -f32::from_bits(1)];
        assert_unit_and_parallel(&smallest, &normalize(&smallest));
    }

    #[test]
    fn normalize_turns_a_single_element_into_its_sign() {
        assert_eq!(normalize(&[0.25]), vec![1.0]);
        assert_eq!(normalize(&[-5.0]), vec![-1.0]);
        assert_eq!(normalize(&[0.0]), vec![0.0]);
    }

    #[test]
    fn normalize_returns_a_vector_with_an_infinity_unchanged() {
        let v = vec![f32::INFINITY, 1.0, -2.0];
        assert_eq!(normalize(&v), v);
        let v = vec![1.0, f32::NEG_INFINITY];
        assert_eq!(normalize(&v), v);
    }

    #[test]
    fn normalize_returns_a_vector_with_a_nan_unchanged_and_spreads_no_nan() {
        let v = vec![3.0_f32, f32::NAN, 4.0];
        let n = normalize(&v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0], 3.0);
        assert!(n[1].is_nan());
        assert_eq!(n[2], 4.0);
    }

    #[test]
    fn normalize_of_an_empty_vector_is_empty() {
        assert!(normalize(&[]).is_empty());
    }

    #[test]
    fn tail_capped_keeps_a_string_of_exactly_the_cap() {
        let exact = "e".repeat(DETAIL_CAP_BYTES);
        assert_eq!(tail_capped(exact.clone()), exact);
    }

    #[test]
    fn tail_capped_drops_only_the_first_byte_at_one_past_the_cap() {
        let over = format!("H{}", "t".repeat(DETAIL_CAP_BYTES));
        let capped = tail_capped(over);
        assert_eq!(capped, "t".repeat(DETAIL_CAP_BYTES));
    }

    #[test]
    fn tail_capped_skips_a_multibyte_char_straddling_the_cut() {
        let over = format!("中{}", "b".repeat(DETAIL_CAP_BYTES - 1));
        assert_eq!(over.len(), DETAIL_CAP_BYTES + 2);
        assert_eq!(tail_capped(over), "b".repeat(DETAIL_CAP_BYTES - 1));
    }

    #[test]
    fn tail_capped_keeps_a_multibyte_char_starting_exactly_at_the_cut() {
        let over = format!("ab中{}", "c".repeat(DETAIL_CAP_BYTES - 3));
        assert_eq!(over.len(), DETAIL_CAP_BYTES + 2);
        let capped = tail_capped(over);
        assert!(capped.starts_with('中'));
        assert_eq!(capped.len(), DETAIL_CAP_BYTES);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn providers_refuse_a_device_this_build_does_not_carry_naming_both_engines() {
        for foreign in [EmbedDevice::DirectMl, EmbedDevice::CoreMl] {
            match providers(foreign) {
                Err(EmbedError::GpuUnavailable { detail }) => {
                    assert!(detail.contains(foreign.label()), "{detail}");
                    assert!(detail.contains(EmbedDevice::WebGpu.label()), "{detail}");
                }
                Err(other) => panic!("{foreign:?}: wrong error {other:?}"),
                Ok(_) => panic!("{foreign:?} must be refused on Linux"),
            }
        }
    }

    #[test]
    fn providers_build_one_provider_for_the_processor_and_the_compiled_device() {
        assert_eq!(providers(EmbedDevice::Cpu).map(|p| p.len()), Ok(1));
        assert_eq!(providers(EmbedDevice::compiled()).map(|p| p.len()), Ok(1));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn providers_refuse_the_broken_test_door() {
        match providers(EmbedDevice::Broken) {
            Err(EmbedError::GpuUnavailable { detail }) => assert!(detail.contains("test door")),
            Err(other) => panic!("wrong error {other:?}"),
            Ok(_) => panic!("the test door must always refuse"),
        }
    }

    #[test]
    fn the_processor_provider_is_always_available() {
        assert_eq!(ensure_provider_available(EmbedDevice::Cpu), Ok(()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_foreign_provider_is_unavailable() {
        assert!(matches!(
            ensure_provider_available(EmbedDevice::DirectMl),
            Err(EmbedError::GpuUnavailable { .. })
        ));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_broken_test_door_is_unavailable() {
        assert!(matches!(
            ensure_provider_available(EmbedDevice::Broken),
            Err(EmbedError::GpuUnavailable { .. })
        ));
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
