//! baijia-suo — A secure Wayland screen locker.
//!
//! This library provides the core functionality for the Baijia-suo
//! (百百家锁, "Hundred-family lock") screen locker.
//!
//! The public surface is intentionally small: the `run()` entry point used
//! by the binary shim, plus the modules exercised by external probe
//! harnesses (`animation`, `render::indicator`, `app::AuthState`,
//! `config`). Everything else is crate-private.

#![deny(unsafe_code)]

pub mod animation;
pub mod app;
pub mod config;
#[allow(unsafe_code)]
pub mod render;

#[allow(unsafe_code)]
mod rng;

mod args;
#[allow(unsafe_code)]
mod auth;
#[allow(unsafe_code)]
mod cli;
#[allow(unsafe_code)]
mod input;
mod password;
#[allow(unsafe_code)]
mod secure;
#[allow(unsafe_code)]
mod wayland;

pub use cli::run;
