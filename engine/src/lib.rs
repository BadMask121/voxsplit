//! VoxSplit engine: offline lead/backing vocal separation with MDX-Net ONNX
//! models. The binary (`main.rs`) is a thin CLI over this library.

pub mod audio;
pub mod cancel;
pub mod device;
pub mod error;
pub mod events;
pub mod model;
pub mod onnx;
pub mod pipeline;
pub mod separator;
pub mod stft;
pub mod timing;
