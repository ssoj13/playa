//! Player and playback events.

use std::path::PathBuf;
use uuid::Uuid;

// === Playback Control ===

#[derive(Clone, Debug)]
pub struct PlayEvent;

#[derive(Clone, Debug)]
pub struct PauseEvent;

#[derive(Clone, Debug)]
pub struct StopEvent;

#[derive(Clone, Debug)]
pub struct TogglePlayPauseEvent;

#[derive(Clone, Debug)]
pub struct SetFrameEvent(pub i32);

#[derive(Clone, Debug)]
pub struct StepForwardEvent;

#[derive(Clone, Debug)]
pub struct StepBackwardEvent;

#[derive(Clone, Debug)]
pub struct StepForwardLargeEvent;

#[derive(Clone, Debug)]
pub struct StepBackwardLargeEvent;

#[derive(Clone, Debug)]
pub struct JumpToStartEvent;

#[derive(Clone, Debug)]
pub struct JumpToEndEvent;

#[derive(Clone, Debug)]
pub struct JumpToPrevEdgeEvent;

#[derive(Clone, Debug)]
pub struct JumpToNextEdgeEvent;

#[derive(Clone, Debug)]
pub struct JogForwardEvent;

#[derive(Clone, Debug)]
pub struct JogBackwardEvent;

#[derive(Clone, Debug)]
pub struct PreviousClipEvent;

#[derive(Clone, Debug)]
pub struct NextClipEvent;

// === FPS Control ===

#[derive(Clone, Debug)]
pub struct IncreaseFPSBaseEvent;

#[derive(Clone, Debug)]
pub struct DecreaseFPSBaseEvent;

// === Play Range ===

#[derive(Clone, Debug)]
pub struct SetPlayRangeStartEvent;

#[derive(Clone, Debug)]
pub struct SetPlayRangeEndEvent;

#[derive(Clone, Debug)]
pub struct ResetPlayRangeEvent;

// === Loop ===

#[derive(Clone, Debug)]
pub struct ToggleLoopEvent;
