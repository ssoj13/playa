//! egui widgets for the `playa-jobs` subsystem.
//!
//! - [`JobsPanel`] — table view with sort, filter, multi-select, action bar.
//!   Renders against a live [`playa_jobs_core::JobQueue`]; returns
//!   [`JobsAction`] enum for the host to dispatch to queue methods.
//! - [`SubmitDialog`] — modal for composing a Seedance generation request
//!   (text-to-video or image-to-video) with live cost preview.
//! - [`prefs::render`] — preferences-panel renderer for
//!   [`playa_jobs_core::JobsSettings`]; pluggable into a [`egui_prefs::PrefsRegistry`].
//!
//! This crate is optional: the host enables it via the `ui` feature on the
//! `playa-jobs` facade. Standalone consumption is also fine — depend
//! directly and call the widgets from any egui app.

#![forbid(unsafe_code)]

pub mod dialog;
pub mod panel;
pub mod prefs;

pub use dialog::{SubmitDialog, SubmitDialogResult, SubmitEndpoint};
pub use panel::{JobsAction, JobsPanel, JobsSortColumn};
