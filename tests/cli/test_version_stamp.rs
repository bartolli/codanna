//! `--version` and `-V` print the crate version plus the build.rs commit
//! stamp when one exists; builds without `.git` print the bare version.

use std::process::Command;

fn version_line(flag: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_codanna"))
        .arg(flag)
        .output()
        .expect("codanna runs");
    assert!(out.status.success(), "{flag} exits 0");
    String::from_utf8(out.stdout)
        .expect("version output is UTF-8")
        .trim_end()
        .to_string()
}

#[test]
fn version_flags_print_version_and_build_stamp() {
    let version = env!("CARGO_PKG_VERSION");
    let expected = match option_env!("CODANNA_GIT_COMMIT") {
        Some(stamp) => format!("codanna {version} ({stamp})"),
        None => format!("codanna {version}"),
    };
    assert_eq!(
        expected,
        format!("codanna {}", env!("CODANNA_VERSION_STRING")),
        "build.rs composes the string from the same stamp"
    );
    for flag in ["--version", "-V"] {
        assert_eq!(
            version_line(flag),
            expected,
            "{flag} prints the composed version"
        );
    }
}
