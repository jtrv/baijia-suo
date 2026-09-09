//! baijia-suo — A secure Wayland screen locker.
//!
//! This library provides the core functionality for the Baijia-suo
//! (百百家锁, "Hundred-family lock") screen locker.
//!
//! The public surface is intentionally small: the `run()` entry point used
//! by the binary shim, plus the modules exercised by external probe
//! harnesses (`animation`, `render::indicator`, `app::AuthState`,
//! `config`). Everything else is crate-private.

pub mod animation;
pub mod app;
pub mod config;
pub mod render;

mod rng;

mod args;
mod auth;
mod cli;
mod input;
mod password;
mod secure;
mod wayland;

pub use cli::run;
