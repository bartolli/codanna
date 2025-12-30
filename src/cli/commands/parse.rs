//! Parse command - output AST as JSONL.

use std::path::{Path, PathBuf};

/// Run parse command to output AST as JSONL
pub fn run(file_path: &Path, output: Option<PathBuf>, max_depth: Option<usize>, all_nodes: bool) {
    use crate::io::parse::execute_parse;

    match execute_parse(file_path, output, max_depth, all_nodes) {
        Ok(()) => {
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            let exit_code = e.exit_code();
            std::process::exit(exit_code as i32);
        }
    }
}
