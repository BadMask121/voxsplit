use crate::events::ErrorCode;

/// Fatal engine errors, each mapped to a protocol error code and exit status
/// (see docs/02-engine-protocol.md).
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    BadArgs(String),
    #[error("{0:#}")]
    Decode(anyhow::Error),
    #[error("{0:#}")]
    ModelLoad(anyhow::Error),
    #[error("{0:#}")]
    Inference(anyhow::Error),
    #[error("{0:#}")]
    Write(anyhow::Error),
    #[error("cancelled")]
    Cancelled,
    #[error("{0:#}")]
    Internal(anyhow::Error),
}

impl EngineError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::BadArgs(_) => ErrorCode::BadArgs,
            Self::Decode(_) => ErrorCode::Decode,
            Self::ModelLoad(_) => ErrorCode::ModelLoad,
            Self::Inference(_) => ErrorCode::Inference,
            Self::Write(_) => ErrorCode::Write,
            Self::Cancelled | Self::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Internal(_) => 1,
            Self::BadArgs(_) => 2,
            Self::Decode(_) => 3,
            Self::ModelLoad(_) => 4,
            Self::Inference(_) => 5,
            Self::Write(_) => 6,
            Self::Cancelled => 130,
        }
    }
}
