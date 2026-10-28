//! VoxSplit engine: offline lead/backing vocal separation with MDX-Net ONNX
//! models. The binary (`main.rs`) is a thin CLI over this library.

pub mod audio;
pub mod cancel;
pub mod error;
pub mod events;
pub mod model;
pub mod separator;
pub mod stft;
