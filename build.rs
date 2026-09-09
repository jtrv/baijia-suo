//! Static completions under `completions/` are updated by hand when flags change.
//! Copy them to `target/completions/` for packagers.

use std::path::PathBuf;

fn main() {
    for completion in ["baijia-suo.bash", "_baijia-suo", "baijia-suo.fish"] {
        println!("cargo:rerun-if-changed=completions/{completion}");
    }

    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
        PathBuf::from,
    );
    let out = target.join("completions");
    if std::fs::create_dir_all(&out).is_err() {
        return; // best-effort: never fail the build over completions
    }
    for completion in ["baijia-suo.bash", "_baijia-suo", "baijia-suo.fish"] {
        let _ = std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("completions")
                .join(completion),
            out.join(completion),
        );
    }
}
