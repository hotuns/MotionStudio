use anyhow::{ensure, Context, Result};
use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use zip::ZipArchive;

const EXE: &str = "Motion Studio.exe";
const REQUIRED: &[&str] = &[
    EXE,
    "resources/motion-engine.exe",
    "resources/SDL3.dll",
    "resources/runtime-manifest.json",
    "drivers/manifest.json",
];
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;

pub fn stage_next_to_app(bytes: &[u8]) -> Result<TempDir> {
    let executable = std::env::current_exe()?;
    let current = executable.parent().context("Missing app directory")?;
    stage(bytes, current.parent().unwrap_or(current))
}

pub(super) fn stage(bytes: &[u8], parent: &Path) -> Result<TempDir> {
    // The updater has already verified the full ZIP and its signed version.
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    ensure!(zip.len() <= 4000, "Too many files in portable archive");
    let mut seen = HashSet::new();
    let mut files = HashSet::new();
    let mut unpacked = 0u64;
    for i in 0..zip.len() {
        let item = zip.by_index(i)?;
        let path = item.name().trim_end_matches('/');
        ensure!(
            item.enclosed_name().is_some()
                && !item.is_symlink()
                && path.split('/').all(safe_segment)
                && seen.insert(path.to_ascii_lowercase()),
            "Unsafe or repeated path in portable archive: {path}"
        );
        if item.is_file() {
            files.insert(path.to_owned());
        }
        unpacked = unpacked.saturating_add(item.size());
        ensure!(
            unpacked <= MAX_UNPACKED,
            "Portable archive exceeds size limit"
        );
    }
    ensure!(
        REQUIRED.iter().all(|name| files.contains(*name)),
        "Incomplete portable archive"
    );
    let staging = tempfile::Builder::new()
        .prefix(".motion-studio-update-")
        .tempdir_in(parent)?;
    zip.extract(staging.path())?;
    Ok(staging)
}

fn safe_segment(segment: &str) -> bool {
    if segment.is_empty()
        || segment.ends_with(['.', ' '])
        || !segment
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b" ._-".contains(&c))
    {
        return false;
    }
    let stem = segment
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    !(matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()))
}

pub fn publish(staging: TempDir, version: &str) -> Result<PathBuf> {
    let parent = staging
        .path()
        .parent()
        .context("Missing destination directory")?;
    let base = format!("MotionStudio-{version}-windows-x64");
    for attempt in 1..=100 {
        let folder = if attempt == 1 {
            base.clone()
        } else {
            format!("{base} ({attempt})")
        };
        let destination = parent.join(folder);
        match fs::symlink_metadata(&destination) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        // On Windows this fails if another process creates the destination first.
        fs::rename(staging.path(), &destination)?;
        return Ok(destination.join(EXE));
    }
    anyhow::bail!("No free directory name for the portable update")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn zip(extra: Option<&str>, omit_last: bool) -> Vec<u8> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let required = &REQUIRED[..REQUIRED.len() - usize::from(omit_last)];
        for name in required.iter().copied().chain(extra) {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_complete_package_without_replacing_existing_version() {
        let parent = tempfile::tempdir().unwrap();
        let existing = parent.path().join("MotionStudio-0.16.1-windows-x64");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join(EXE), "previous").unwrap();
        let staged = stage(&zip(None, false), parent.path()).unwrap();
        let executable = publish(staged, "0.16.1").unwrap();
        assert_eq!(fs::read_to_string(existing.join(EXE)).unwrap(), "previous");
        assert!(executable
            .parent()
            .unwrap()
            .ends_with("MotionStudio-0.16.1-windows-x64 (2)"));
        assert_eq!(fs::read_to_string(&executable).unwrap(), EXE);
        assert!(executable.parent().unwrap().join(REQUIRED[1]).is_file());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 2);
    }

    #[test]
    fn rejects_incomplete_or_unsafe_archives_before_writing() {
        let parent = tempfile::tempdir().unwrap();
        for path in [
            "../escape.exe",
            "resources/../../escape.exe",
            "resources/data:stream",
            "resources/NUL.txt",
            "resources/SDL3.DLL",
            "resources/test.",
        ] {
            assert!(
                stage(&zip(Some(path), false), parent.path()).is_err(),
                "{path}"
            );
        }
        assert!(stage(&zip(None, true), parent.path()).is_err());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}
