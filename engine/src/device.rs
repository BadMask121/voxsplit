//! Hardware-accelerator selection. Each platform gets an ordered list of
//! execution providers to try; CPU is always the last resort.

use std::path::Path;

use clap::ValueEnum;
#[allow(unused_imports)]
use ort::ep::{self, ExecutionProvider, ExecutionProviderDispatch};

use crate::events::DeviceInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Device {
    Auto,
    Coreml,
    Cuda,
    Directml,
    Cpu,
}

/// One execution provider to attempt, in priority order.
pub struct Candidate {
    pub name: &'static str,
    pub dispatch: ExecutionProviderDispatch,
}

/// Providers to try, in order, for the requested device. An explicit device
/// yields exactly one candidate (so failure is an error, not a fallback).
pub fn candidates(device: Device, coreml_cache: Option<&Path>) -> Result<Vec<Candidate>, String> {
    let cpu = || Candidate {
        name: "cpu",
        dispatch: cpu_ep(),
    };
    let mut list = Vec::new();
    match device {
        Device::Auto => {
            #[cfg(target_os = "macos")]
            list.push(coreml(coreml_cache));
            #[cfg(feature = "cuda")]
            list.push(cuda());
            #[cfg(windows)]
            list.push(directml());
            list.push(cpu());
        }
        Device::Cpu => list.push(cpu()),
        Device::Coreml => {
            #[cfg(target_os = "macos")]
            list.push(coreml(coreml_cache));
        }
        Device::Cuda => {
            #[cfg(feature = "cuda")]
            list.push(cuda());
        }
        Device::Directml => {
            #[cfg(windows)]
            list.push(directml());
        }
    }
    let _ = coreml_cache;
    if list.is_empty() {
        return Err(format!("{device:?} is not supported by this build").to_lowercase());
    }
    Ok(list)
}

/// MDX-Net keeps several full-resolution (dim_f x dim_t) activations alive, so
/// the CPU path peaks around 4.5 GB. Turning off the arena (and, in `onnx.rs`,
/// memory-pattern planning) frees buffers as they're consumed (~7% lower).
fn cpu_ep() -> ExecutionProviderDispatch {
    ep::CPU::default().with_arena_allocator(false).build()
}

#[cfg(target_os = "macos")]
fn coreml(cache: Option<&Path>) -> Candidate {
    use ep::coreml::{ComputeUnits, ModelFormat};
    // MLProgram covers more operators than the legacy NeuralNetwork format and
    // lets Core ML schedule across the Neural Engine, GPU and CPU.
    let mut ep = ep::CoreML::default()
        .with_model_format(ModelFormat::MLProgram)
        .with_compute_units(ComputeUnits::All)
        .with_static_input_shapes(true);
    if let Some(dir) = cache {
        ep = ep.with_model_cache_dir(dir.display());
    }
    Candidate {
        name: "coreml",
        dispatch: ep.build().error_on_failure(),
    }
}

#[cfg(feature = "cuda")]
fn cuda() -> Candidate {
    Candidate {
        name: "cuda",
        dispatch: ep::CUDA::default().build().error_on_failure(),
    }
}

#[cfg(windows)]
fn directml() -> Candidate {
    Candidate {
        name: "directml",
        dispatch: ep::DirectML::default().build().error_on_failure(),
    }
}

/// What this binary was built with and what the linked ONNX Runtime reports.
pub fn probe() -> Vec<DeviceInfo> {
    #[allow(unused_mut)]
    let mut devices = vec![
        DeviceInfo {
            name: "coreml",
            compiled: cfg!(target_os = "macos"),
            available: false,
        },
        DeviceInfo {
            name: "cuda",
            compiled: cfg!(feature = "cuda"),
            available: false,
        },
        DeviceInfo {
            name: "directml",
            compiled: cfg!(windows),
            available: false,
        },
        DeviceInfo {
            name: "cpu",
            compiled: true,
            available: true,
        },
    ];
    #[cfg(target_os = "macos")]
    {
        devices[0].available = ep::CoreML::is_available().unwrap_or(false);
    }
    #[cfg(feature = "cuda")]
    {
        devices[1].available = ep::CUDA::is_available().unwrap_or(false);
    }
    #[cfg(windows)]
    {
        devices[2].available = ep::DirectML::is_available().unwrap_or(false);
    }
    devices
}
