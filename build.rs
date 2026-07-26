//! Stamps the building commit into the binary as `CODANNA_GIT_COMMIT`.
//!
//! Release tarballs carry no `.git`, so the variable is simply absent there
//! and `option_env!` yields `None` — which is itself a signal: no hash means
//! the binary was not built from a work tree.

use std::process::Command;

fn main() {
    // Without this the stamp survives a checkout and starts lying.
    for path in [".git/HEAD", ".git/index"] {
        if std::path::Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    if let Some(stamp) = git_stamp() {
        println!("cargo:rustc-env=CODANNA_GIT_COMMIT={stamp}");
    }
}

/// Short commit, suffixed `-dirty` when tracked files differ from it.
///
/// The suffix is load-bearing: development binaries are routinely built from
/// modified trees, and a bare hash there names a commit whose code is not
/// what ran.
fn git_stamp() -> Option<String> {
    let head = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !head.status.success() {
        return None;
    }
    let commit = String::from_utf8(head.stdout).ok()?.trim().to_string();
    if commit.is_empty() {
        return None;
    }

    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false);

    Some(if dirty {
        format!("{commit}-dirty")
    } else {
        commit
    })
}
