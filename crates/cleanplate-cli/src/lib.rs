//! cleanplate CLI internals (IO only; every algorithm lives in cleanplate-core).
pub mod batch;
pub mod ffmpeg;
pub mod still_io;
pub mod video_io;

use std::path::{Path, PathBuf};

/// What happened to one file (errors are the `Err` of the cleaning functions).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Cleaned,
    /// No mark found with confidence: nothing was written. The reason, for the user.
    Refused(String),
}

/// Where an output is written until it is complete. It is renamed into place on success, so a crash or Ctrl-C never
/// leaves a partial file under the final name (only this hidden one, which the next run overwrites).
pub fn partial_path(out: &Path) -> PathBuf {
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
    out.with_file_name(format!(".{stem}.partial.{ext}"))
}

/// Move a finished partial output into place.
pub fn finish(partial: &Path, out: &Path) -> std::io::Result<()> {
    std::fs::rename(partial, out)
}
