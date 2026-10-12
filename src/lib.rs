mod catalog;
mod device;
mod embedder;
mod error;

pub use catalog::catalog;
pub use device::EmbedDevice;
pub use embedder::Embedder;
pub use error::EmbedError;

pub const DIMS: usize = 768;
