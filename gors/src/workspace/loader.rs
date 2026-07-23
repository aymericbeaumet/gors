use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::compiler::input::{
    PackageInputManifest, PackageKey, ProgramInput, SourceFileInput, WorkspaceKey,
};

use super::{LoadError, LoadedProgram, PathExpectation};

struct SelectedFile {
    canonical_path: PathBuf,
    logical_path: Arc<str>,
}

/// Discover one source file or a directory's immediate eligible Go files.
///
/// Filesystem discovery never scans or parses Go text and does not resolve
/// imports. Directory selection is non-recursive. `workspace` is a stable
/// logical identity selected by the caller; it is never derived from `path`.
pub fn load_program(
    workspace: WorkspaceKey,
    path: impl AsRef<Path>,
) -> Result<LoadedProgram, LoadError> {
    let invocation_path = path.as_ref();
    ensure_utf8(invocation_path)?;
    let canonical = canonicalize(invocation_path)?;
    ensure_utf8(&canonical)?;
    let metadata = std::fs::metadata(&canonical)
        .map_err(|error| LoadError::io("inspect source path", canonical.clone(), error))?;
    if metadata.is_file() {
        let selected = select_explicit_files([canonical])?;
        return load_selected(workspace, selected, Arc::from([]));
    }
    if metadata.is_dir() {
        let selected = select_directory(&canonical)?;
        return load_selected(workspace, selected, Arc::from([canonical]));
    }
    Err(LoadError::InvalidPathKind {
        path: canonical,
        expected: PathExpectation::FileOrDirectory,
    })
}

/// Load an explicit source-file set in deterministic canonical order.
///
/// A single argument keeps [`load_program`] semantics and may name a
/// directory. Two or more arguments must be eligible files in one directory.
/// `workspace` is caller-owned and independent of the files' physical paths.
pub fn load_program_files<P: AsRef<Path>>(
    workspace: WorkspaceKey,
    paths: &[P],
) -> Result<LoadedProgram, LoadError> {
    let Some(first) = paths.first() else {
        return Err(LoadError::NoInputPaths);
    };
    if paths.len() == 1 {
        return load_program(workspace, first);
    }

    let mut canonical = Vec::with_capacity(paths.len());
    for path in paths {
        let path = path.as_ref();
        ensure_utf8(path)?;
        let path = canonicalize(path)?;
        ensure_utf8(&path)?;
        canonical.push(path);
    }
    let selected = select_explicit_files(canonical)?;
    load_selected(workspace, selected, Arc::from([]))
}

fn select_directory(directory: &Path) -> Result<Vec<SelectedFile>, LoadError> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| LoadError::io("read directory", directory.to_path_buf(), error))?;
    let mut selected = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| LoadError::io("read directory", directory.to_path_buf(), error))?;
        let path = entry.path();
        let Some(logical_path) = eligible_filename(&path)? else {
            continue;
        };
        let metadata = std::fs::metadata(&path)
            .map_err(|error| LoadError::io("inspect source file", path.clone(), error))?;
        if !metadata.is_file() {
            continue;
        }
        let canonical_path = canonicalize(&path)?;
        ensure_utf8(&canonical_path)?;
        selected.push(SelectedFile {
            canonical_path,
            logical_path,
        });
    }
    if selected.is_empty() {
        return Err(LoadError::NoGoFiles {
            directory: directory.to_path_buf(),
        });
    }
    canonicalize_selected_order(&mut selected)?;
    Ok(selected)
}

fn select_explicit_files(
    canonical_paths: impl IntoIterator<Item = PathBuf>,
) -> Result<Vec<SelectedFile>, LoadError> {
    let mut selected = Vec::new();
    for canonical_path in canonical_paths {
        let metadata = std::fs::metadata(&canonical_path)
            .map_err(|error| LoadError::io("inspect source file", canonical_path.clone(), error))?;
        if !metadata.is_file() {
            return Err(LoadError::InvalidPathKind {
                path: canonical_path,
                expected: PathExpectation::SourceFile,
            });
        }
        let logical_path =
            eligible_filename(&canonical_path)?.ok_or_else(|| LoadError::InvalidPathKind {
                path: canonical_path.clone(),
                expected: PathExpectation::EligibleGoSource,
            })?;
        selected.push(SelectedFile {
            canonical_path,
            logical_path,
        });
    }
    canonicalize_selected_order(&mut selected)?;
    require_shared_directory(&selected)?;
    Ok(selected)
}

fn canonicalize_selected_order(selected: &mut [SelectedFile]) -> Result<(), LoadError> {
    selected.sort_by(|left, right| {
        left.canonical_path
            .cmp(&right.canonical_path)
            .then_with(|| left.logical_path.cmp(&right.logical_path))
    });
    if let Some(path) = selected.windows(2).find_map(|pair| {
        let [left, right] = pair else {
            return None;
        };
        (left.canonical_path == right.canonical_path).then(|| left.canonical_path.clone())
    }) {
        return Err(LoadError::DuplicateSourceFile { path });
    }
    selected.sort_by(|left, right| left.logical_path.cmp(&right.logical_path));
    Ok(())
}

fn require_shared_directory(selected: &[SelectedFile]) -> Result<(), LoadError> {
    let Some(first) = selected.first() else {
        return Err(LoadError::NoInputPaths);
    };
    let expected = parent_directory(&first.canonical_path)?;
    for file in selected.iter().skip(1) {
        let found = parent_directory(&file.canonical_path)?;
        if found != expected {
            return Err(LoadError::SourceFilesFromDifferentDirectories {
                expected_directory: expected.to_path_buf(),
                file: file.canonical_path.clone(),
                found_directory: found.to_path_buf(),
            });
        }
    }
    Ok(())
}

fn load_selected(
    workspace: WorkspaceKey,
    selected: Vec<SelectedFile>,
    watched_directories: Arc<[PathBuf]>,
) -> Result<LoadedProgram, LoadError> {
    let mut files = Vec::with_capacity(selected.len());
    let mut primary_diagnostic_path = None;
    for selected_file in selected {
        let display_path = path_to_utf8(&selected_file.canonical_path)?;
        let bytes = std::fs::read(&selected_file.canonical_path).map_err(|error| {
            LoadError::io(
                "read source file",
                selected_file.canonical_path.clone(),
                error,
            )
        })?;
        let source = String::from_utf8(bytes).map_err(|_| LoadError::NonUtf8Source {
            path: selected_file.canonical_path.clone(),
        })?;
        primary_diagnostic_path.get_or_insert_with(|| Arc::clone(&display_path));
        files.push(SourceFileInput::from_source(
            selected_file.logical_path,
            display_path,
            source,
        )?);
    }

    let Some(primary_diagnostic_path) = primary_diagnostic_path else {
        return Err(LoadError::NoInputPaths);
    };
    let package_key = PackageKey::command_line();
    let package = PackageInputManifest::new(package_key, files)?;
    let input = ProgramInput::standalone(workspace, package)?;
    Ok(LoadedProgram::new(
        input,
        watched_directories,
        primary_diagnostic_path,
    ))
}

fn eligible_filename(path: &Path) -> Result<Option<Arc<str>>, LoadError> {
    if path.extension() != Some(OsStr::new("go")) {
        return Ok(None);
    }
    let filename =
        path.file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| LoadError::NonUtf8Path {
                path: path.to_path_buf(),
            })?;
    let eligible =
        !filename.starts_with('.') && !filename.starts_with('_') && !filename.ends_with("_test.go");
    Ok(eligible.then(|| Arc::from(filename)))
}

fn canonicalize(path: &Path) -> Result<PathBuf, LoadError> {
    std::fs::canonicalize(path)
        .map_err(|error| LoadError::io("canonicalize source path", path.to_path_buf(), error))
}

fn ensure_utf8(path: &Path) -> Result<(), LoadError> {
    path.to_str()
        .map(|_| ())
        .ok_or_else(|| LoadError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

fn path_to_utf8(path: &Path) -> Result<Arc<str>, LoadError> {
    path.to_str()
        .map(Arc::from)
        .ok_or_else(|| LoadError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

fn parent_directory(path: &Path) -> Result<&Path, LoadError> {
    path.parent().ok_or_else(|| LoadError::InvalidPathKind {
        path: path.to_path_buf(),
        expected: PathExpectation::SourceFile,
    })
}
