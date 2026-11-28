use std::{fs, path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_codanna"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn deleted_import_does_not_capture_another_roots_export() {
    for extension in ["ts", "js"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::create_dir(root.join(".codanna")).unwrap();
        fs::write(root.join(".codanna/settings.toml"), format!(
            "workspace_root = {}\n[indexing]\nindexed_paths = [\"repo_a\", \"repo_b\"]\n[semantic_search]\nenabled = false\n",
            serde_json::to_string(&root).unwrap()
        )).unwrap();
        for repo in ["repo_a", "repo_b"] {
            fs::create_dir(root.join(repo)).unwrap();
            fs::write(
                root.join(repo).join(format!("target.{extension}")),
                "export function sharedTarget() { return 42; }\n",
            )
            .unwrap();
        }
        fs::write(root.join("repo_a").join(format!("caller.{extension}")),
            "import { sharedTarget } from './target';\nexport function entry() { return sharedTarget(); }\nexport const arrowEntry = () => sharedTarget();\n").unwrap();
        run(&root, &["index", "--no-progress"]);
        let edges = run(&root, &["dump", "--edges", "--relation", "calls"]);
        let calls: Vec<_> = edges
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|row| {
                row["type"] == "result"
                    && matches!(
                        row["data"]["from"]["name"].as_str(),
                        Some("entry" | "arrowEntry")
                    )
            })
            .collect();
        let callers: std::collections::BTreeSet<_> = calls
            .iter()
            .map(|row| row["data"]["from"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            callers,
            ["entry", "arrowEntry"].into_iter().collect(),
            "valid {extension} imports must resolve"
        );
        for row in calls {
            assert!(
                row["data"]["to"]["file_path"]
                    .as_str()
                    .unwrap()
                    .contains("repo_a/target")
            );
        }
        fs::remove_file(root.join("repo_a").join(format!("target.{extension}"))).unwrap();
        for args in [
            vec!["index", "--no-progress"],
            vec!["index", "--force", "--no-progress"],
        ] {
            run(&root, &args);
            let edges = run(&root, &["dump", "--edges", "--relation", "calls"]);
            assert!(
                !edges
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .any(|row| row["type"] == "result"),
                "unresolved {extension} import must not capture repo_b: {edges}"
            );
        }
    }
}
