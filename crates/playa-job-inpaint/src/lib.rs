//! Inpaint provider for [`playa_jobs::JobQueue`].
//!
//! # Vendor and surface
//!
//! Targets fal.ai's hosted Flux Pro v1.1 inpainting endpoint
//! (`fal-ai/flux-pro/v1.1/inpainting`). The fal queue API returns
//! `status_url` / `response_url` / `cancel_url` in the submit response —
//! the provider follows them rather than constructing paths, so a
//! fal-side URL change doesn't break the integration.
//!
//! Auth: `Authorization: Key <FAL_KEY>` — same as
//! [`playa-job-seedance`](https://docs.rs/playa-job-seedance).
//!
//! # Inputs
//!
//! - `image_url`: base image — HTTP URL OR `data:image/png;base64,...`
//!   data URL. Both forms accepted by fal.
//! - `mask_url`: mask image (white = inpaint, black = preserve) — same
//!   format options as `image_url`.
//! - `prompt`: replacement description.
//! - `seed`: resolved u64 (NOT "auto" — submit-side resolution lives in
//!   `playa-app`'s submit dialog so the `Generation` record can capture
//!   the concrete seed for reproducibility).
//! - Optional: `num_inference_steps`, `guidance_scale`, `safety_tolerance`
//!   (forwarded verbatim to fal).
//!
//! # Crash-resume contract
//!
//! Before `Submitting → AwaitingProvider` the provider calls
//! [`playa_jobs::JobContext::persist_param`] for `request_id`,
//! `status_url`, and `response_url`. [`InpaintProvider::resume`] reads
//! those back and re-enters the poll loop without re-billing fal.
//!
//! # Cost
//!
//! Flux Pro v1.1 inpainting bills per-megapixel. At time of writing fal
//! charges ~$0.05 / megapixel. A 1024×1024 (1 MP) inpaint is ~$0.05.
//! [`InpaintProvider::estimate_cost_usd`] honours `image_size` /
//! `width` / `height` in params when present; absent fields yield
//! `None` (queue treats unknown estimates as $0 against the daily cap,
//! favouring submit over false rejection).

#![forbid(unsafe_code)]

pub mod http;
pub mod provider;

pub use http::{FalHttp, UreqFalHttp};
pub use provider::{InpaintEndpoint, InpaintProvider, InpaintProviderConfig, kinds};
