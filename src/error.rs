use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, PartialEq, Error)]
pub enum EmbedError {
    #[error("embedding model file missing at {path}")]
    ModelMissing { path: PathBuf },
    #[error("GPU unavailable: {detail}")]
    GpuUnavailable { detail: String },
    #[error("embedding model failed to load: {detail}")]
    LoadFailed { detail: String },
    #[error("embedding inference failed: {detail}")]
    Inference { detail: String },
}
