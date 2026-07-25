//! Build script — captures the git commit hash and build timestamp so the
//! runtime can stamp them into the log on startup. This is critical for
//! diagnosing "stale .zip" issues where the user thinks they're running
//! the latest build but actually have an older .exe cached locally.
//!
//! We do NOT fail the build if git is unavailable — we just emit
//! "<unknown>" for the commit hash. The whole point is that the build
//! still succeeds in environments without git (e.g. CI sandboxes), and
//! the "<unknown>" stamp still tells us SOMETHING (it means the binary
//! was built in a non-git context).

use std::process::Command;

fn main() {
    // Re-run if HEAD changes so local rebuilds after commits pick up the
    // new hash. We don't rerun on every .git change because that would
    // force a full rebuild for things like reflog updates.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let build_time = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let full_version = if git_dirty {
        format!("{}-dirty", git_hash)
    } else {
        git_hash.clone()
    };

    println!("cargo:rustc-env=LEAFISH_BUILD_GIT_HASH={}", full_version);
    println!("cargo:rustc-env=LEAFISH_BUILD_TIME={}", build_time);
    println!("cargo:rustc-env=LEAFISH_BUILD_VERSION=leafish-{}", full_version);
}
