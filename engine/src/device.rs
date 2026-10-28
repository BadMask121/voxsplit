//! Hardware-accelerator selection. Each platform gets an ordered list of
//! execution providers to try; CPU is always the last resort.

use std::path::Path;

use clap::ValueEnum;
#[allow(unused_imports)]
use ort::ep::{self, ExecutionProvider, ExecutionProviderDispatch};

use crate::events::DeviceInfo;
use crate::model::ComputeUnits;

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
pub fn candidates(
    device: Device,
    units: ComputeUnits,
    coreml_cache: Option<&Path>,
) -> Result<Vec<Candidate>, String> {
    let cpu = || Candidate {
        name: "cpu",
        dispatch: cpu_ep(),
    };
    let mut list = Vec::new();
    match device {
        Device::Auto => {
            #[cfg(target_os = "macos")]
            list.push(coreml(units, coreml_cache));
            #[cfg(feature = "cuda")]
            list.push(cuda());
            #[cfg(windows)]
            list.push(directml());
            list.push(cpu());
        }
        Device::Cpu => list.push(cpu()),
        Device::Coreml => {
            #[cfg(target_os = "macos")]
            list.push(coreml(units, coreml_cache));
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
    let _ = (units, coreml_cache);
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
fn coreml(units: ComputeUnits, cache: Option<&Path>) -> Candidate {
    use ep::coreml::{ComputeUnits as Units, ModelFormat};
    // MLProgram covers more operators than the legacy NeuralNetwork format. Which
    // units are fastest is per model: MDX-Net does well across all of them, while
    // the RoFormer's attention runs ~10x faster pinned to the GPU than on the ANE.
    let units = match units {
        ComputeUnits::All => Units::All,
        ComputeUnits::Gpu => Units::CPUAndGPU,
        ComputeUnits::Ane => Units::CPUAndNeuralEngine,
        ComputeUnits::Cpu => Units::CPUOnly,
    };
    let mut ep = ep::CoreML::default()
        .with_model_format(ModelFormat::MLProgram)
        .with_compute_units(units)
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

/// The device a run will most likely use, for estimates made before any model
/// loads. `auto` is the first accelerator this build has; CPU otherwise.
pub fn expected_name(device: Device) -> &'static str {
    match device {
        Device::Cpu => "cpu",
        Device::Coreml => "coreml",
        Device::Cuda => "cuda",
        Device::Directml => "directml",
        Device::Auto => probe()
            .iter()
            .find(|d| d.available && d.name != "cpu")
            .map_or("cpu", |d| d.name),
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
