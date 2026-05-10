//! Seedance 2.0 image-to-video provider for [`playa_jobs::JobQueue`].
//!
//! # Vendor and surface
//!
//! Targets fal.ai's hosted Seedance: model id
//! `bytedance/seedance-2.0/image-to-video`. fal's queue API returns the URLs
//! to follow (`status_url`, `response_url`, `cancel_url`) in the submit
//! response — this provider follows them rather than constructing paths, so
//! a fal-side URL-shape change does not break the integration.
//!
//! Auth header: `Authorization: Key <FAL_KEY>` (not "Bearer" — verified
//! against <https://fal.ai/docs/model-apis/model-endpoints/queue>; the model
//! playground page suggests "Bearer" but is misleading).
//!
//! # Crash-resume contract
//!
//! Before the `Submitting → AwaitingProvider` transition the provider calls
//! [`playa_jobs::JobContext::persist_param`] for `request_id`, `status_url`,
//! and `response_url`. If the process restarts mid-poll, [`Self::resume`]
//! reads those back and re-enters the poll loop without re-billing the API.
//!
//! # Cost
//!
//! At the time of writing fal charges $0.3024 / second (standard tier) for
//! the image-to-video model. A 5-second 720p clip is ~$1.50. Always honour
//! the cancel token between long ops — abandoning a job mid-run still
//! charges for whatever fal completed by the time fal sees the cancel.

#![forbid(unsafe_code)]

pub mod http;
pub mod params;
pub mod provider;

pub use http::{FalHttp, UreqFalHttp};
pub use params::{SeedanceImageToVideoParams, SeedanceTextToVideoParams};
pub use provider::{SeedanceEndpoint, SeedanceProvider, SeedanceProviderConfig, kinds};
