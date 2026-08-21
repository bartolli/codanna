//! `codanna dump` command: stream the index to stdout.

use std::io::{self, BufWriter, Write};

use crate::config::Settings;
use crate::dump::{DumpError, DumpFilter, DumpStamp, write_dump};
use crate::indexing::facade::IndexFacade;
use crate::io::ExitCode;
use crate::storage::IndexMetadata;

pub fn run(indexer: &IndexFacade, config: &Settings, filter: &DumpFilter) -> ExitCode {
    let stamp = IndexMetadata::load(&config.index_path)
        .ok()
        .map(|meta| DumpStamp {
            emission_version: meta.emission_version,
            builder_commit: meta.builder_commit,
        })
        .unwrap_or_default();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let result = write_dump(indexer, stamp, filter, &mut out).and_then(|summary| {
        out.flush()?;
        Ok(summary)
    });
    match result {
        Ok(_) => ExitCode::Success,
        Err(DumpError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::Success,
        Err(DumpError::Io(e)) => {
            eprintln!("Error: dump write failed: {e}");
            ExitCode::IoError
        }
        Err(DumpError::Storage(e)) => {
            eprintln!("Error: dump read failed: {e}");
            ExitCode::IndexCorrupted
        }
        Err(DumpError::Json(e)) => {
            eprintln!("Error: dump serialization failed: {e}");
            ExitCode::GeneralError
        }
    }
}
