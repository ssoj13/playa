//! Encode dialog events.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct EncodeStartEvent {
    pub output_path: PathBuf,
    pub codec: String,
}

#[derive(Clone, Debug)]
pub struct EncodeCancelEvent;

#[derive(Clone, Debug)]
pub struct EncodeProgressEvent {
    pub frame: i32,
    pub total: i32,
}

#[derive(Clone, Debug)]
pub struct EncodeCompleteEvent(pub PathBuf);
