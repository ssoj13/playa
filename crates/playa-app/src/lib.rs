//! Playa desktop host — [`PlayaApp`](crate::app::PlayaApp), orchestration,
//! [`main_events`] routing, CLI, prefs paths, REST server.

#![allow(clippy::too_many_arguments)]
#![allow(clippy::module_inception)]

pub mod app;
pub mod cli;
pub mod config;
pub mod main_events;
pub mod runner;
pub mod server;
pub mod shell;

pub use runner::run_app;
