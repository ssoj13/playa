//! Long-running IO job queue for playa.
//!
//! # What this crate is for
//!
//! Tasks that run for *minutes* against external services and must survive
//! application restarts: video-generation API calls (e.g. Seedance), ffmpeg
//! encodes that the user wants tracked centrally, large file imports.
//!
//! # What it is **not** for
//!
//! CPU-bound frame loads / compose work — those live on
//! `playa_engine::core::workers::Workers`. Mixing minute-long IO into the
//! frame-decode pool would starve scrubbing.
//!
//! # Architecture
//!
//! ```text
//!                ┌─────────────────────────────────────────┐
//!  submit() ──►  │  JobQueue (HashMap<JobId, Job> + locks) │
//!                │  ─────────────────────────────────────  │
//!                │   work-queue (Mutex<VecDeque<JobId>>)   │ ◄──── workers wait
//!                │   listeners (subscribers)               │      on Condvar
//!                │   updater thread (drains UpdateMsg)     │
//!                └─────────────────────────────────────────┘
//!                              │
//!                              ▼
//!     N=max(2, ncpu/4) worker threads:
//!       loop { id = work_queue.pop(); provider.run(ctx, params) ... }
//! ```
//!
//! Providers (e.g. Seedance, ffmpeg-encode) live in their **own** crates and
//! register at app boot via [`JobQueue::register_provider`]. This crate stays
//! free of HTTP-client deps; providers pull whatever they need.
//!
//! # Persistence
//!
//! Behind the `persist` cargo feature (on by default), every state change
//! appends a JSONL entry to the configured log path. On boot,
//! [`JobQueue::replay_persisted`] reconstructs the in-memory job map from the
//! log. Crucial for recovering a Seedance task whose `task_id` would otherwise
//! be lost if the app crashed between submit and download — the persist write
//! happens **before** the state transition that creates the wait, so credits
//! are never abandoned.

#![forbid(unsafe_code)]

pub mod cancel;
pub mod event;
pub mod job;
#[cfg(feature = "persist")]
pub mod persist;
pub mod provider;
pub mod queue;
pub mod secret;

pub use cancel::CancelToken;
pub use event::JobEvent;
pub use job::{Job, JobError, JobId, JobProgress, JobState};
pub use provider::{JobContext, JobProvider};
pub use queue::{JobQueue, JobQueueConfig};
