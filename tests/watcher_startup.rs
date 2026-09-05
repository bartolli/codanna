#![cfg(target_os = "macos")]

use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tantivy::{Index, Term, collector::Count, query::TermQuery, schema::IndexRecordOption};

#[test]
#[ignore = "creates 5,000 directories; run explicitly to check FSEvents scaling and delivery"]
fn watches_large_tree_and_preserves_ignore_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join(".codanna")).unwrap();
    fs::write(root.join(".codanna/settings.toml"), format!(
        "workspace_root = {}\n[indexing]\nindexed_paths = [\"src\"]\n[semantic_search]\nenabled = false\n",
        serde_json::to_string(&root).unwrap()
    )).unwrap();
    for i in 0..5000 {
        let path = root.join(format!("src/d{i}"));
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("seed.py"),
            format!("def seed_{i}():\n    return {i}\n"),
        )
        .unwrap();
    }
    // Direct root files must not suppress the native root watch just because
    // their parent is already in the logical directory registry.
    fs::write(root.join("src/seed.py"), "def root_seed():\n    return 0\n").unwrap();
    fs::write(root.join("src/.codannaignore"), "ignored/\n").unwrap();
    let binary = env!("CARGO_BIN_EXE_codanna");
    assert!(
        Command::new(binary)
            .current_dir(&root)
            .args(["index", "--no-progress"])
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let index = Index::open_in_dir(root.join(".codanna/index/tantivy")).unwrap();
    let reader = index.reader().unwrap();
    let name_field = index.schema().get_field("name").unwrap();
    let contains = |name: &str| {
        reader.reload().unwrap();
        reader
            .searcher()
            .search(
                &TermQuery::new(
                    Term::from_field_text(name_field, name),
                    IndexRecordOption::Basic,
                ),
                &Count,
            )
            .unwrap()
            > 0
    };
    let wait_for = |condition: &dyn Fn() -> bool| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(100));
        }
        false
    };
    let log = root.join("watcher.log");
    let mut child = Command::new(binary)
        .current_dir(&root)
        .args(["serve", "--watch"])
        .env("RUST_LOG", "codanna::watcher::unified=info")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let ready = wait_for(&|| {
        fs::read_to_string(&log)
            .unwrap()
            .contains("[watcher] started")
    });
    let ready_elapsed = start.elapsed();
    fs::create_dir(root.join("src/ignored")).unwrap();
    fs::write(
        root.join("src/ignored/skip.py"),
        "def ignored_probe():\n    return 0\n",
    )
    .unwrap();
    fs::create_dir(root.join("src/new_subtree")).unwrap();
    let source = root.join("src/new_subtree/created.py");
    fs::write(&source, "def created_probe():\n    return 1\n").unwrap();
    let created = wait_for(&|| contains("created_probe"));
    fs::write(&source, "def edited_probe():\n    return 2\n").unwrap();
    let edited = wait_for(&|| contains("edited_probe") && !contains("created_probe"));
    fs::remove_file(&source).unwrap();
    let deleted = wait_for(&|| !contains("edited_probe"));
    let ignored = contains("ignored_probe");
    let _ = child.kill();
    child.wait().unwrap();
    assert!(
        ready && created && edited && deleted && !ignored,
        "ready={ready} created={created} edited={edited} deleted={deleted} ignored={ignored}: {}",
        fs::read_to_string(log).unwrap()
    );
    println!(
        "5,000-directory readiness: {ready_elapsed:?}; create/edit/delete and ignore checks passed"
    );
}
