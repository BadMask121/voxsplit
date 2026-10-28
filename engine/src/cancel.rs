//! Cooperative cancellation: SIGINT/SIGTERM set a flag, and on Unix the engine
//! also stops if its parent (the DAW or app) dies and it gets re-parented to
//! launchd, so a crashed host never leaves an orphaned job burning the GPU.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::EngineError;

#[derive(Clone)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    #[cfg(unix)]
    parent: u32,
}

impl Cancel {
    /// Install the signal handler. Call once, early in `main`.
    pub fn install() -> anyhow::Result<Self> {
        let cancel = Self::detached();
        let flag = cancel.flag.clone();
        ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst))?;
        Ok(cancel)
    }

    /// A token with no signal handler, for tests and library use.
    pub fn detached() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            #[cfg(unix)]
            parent: std::os::unix::process::parent_id(),
        }
    }

    pub fn trigger(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        if self.flag.load(Ordering::SeqCst) {
            return true;
        }
        #[cfg(unix)]
        {
            // Started by launchd directly (parent 1): nothing to watch.
            if self.parent != 1 && std::os::unix::process::parent_id() == 1 {
                return true;
            }
        }
        false
    }

    pub fn check(&self) -> Result<(), EngineError> {
        if self.is_cancelled() { Err(EngineError::Cancelled) } else { Ok(()) }
    }
}
