//! Generate shell completions at build time.
//!
//! `clap_complete` introspects the same `Args`/`Command` the binary parses
//! with, so the completions never drift from the flags. It's a build-only
//! dependency — nothing here is linked into the shipped binary. The scripts
//! land under `target/completions/` — build output, next to the binary
//! packagers install alongside them (`baijia-suo.bash`, `_baijia-suo`,
//! `baijia-suo.fish`).

use std::path::PathBuf;

// Pull in the argument definitions. `src/args.rs` depends only on clap, so it
// compiles standalone here without dragging in the rest of the crate.
include!("src/args.rs");

use clap::CommandFactory;
use clap_complete::{generate_to, Shell};

fn main() {
    println!("cargo:rerun-if-changed=src/args.rs");

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    );
    let out = target.join("completions");
    if std::fs::create_dir_all(&out).is_err() {
        return; // best-effort: never fail the build over completions
    }
    let mut cmd = Args::command();
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let _ = generate_to(shell, &mut cmd, "baijia-suo", &out);
    }
}
