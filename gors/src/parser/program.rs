//! Owned multi-file and import-resolved parser inputs.
//!
//! Every file is parsed independently. Package composition records immutable
//! file snapshots; it never concatenates declarations into a synthetic AST.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use super::import_path::{decode_and_validate, validate};
use super::{FileParseError, ImportPathIssue, InvalidImportPathError, ParsedFile, ParsedFileError};

/// Error type for path and program parsing failures.
#[derive(Debug)]
pub enum PathParseError {
    /// An I/O error occurred (file not found, permission denied, etc.).
    IoError(String),
    /// A source file failed parser validation.
    ParserError(FileParseError),
    /// An import literal decoded to an unsafe or non-canonical package path.
    InvalidImportPath(InvalidImportPathError),
    /// No eligible Go files were found.
    NoGoFiles(String),
    /// Files selected for one package used different package clauses.
    PackageMismatch {
        expected: String,
        found: String,
        file: String,
    },
    /// The same physical file was supplied more than once.
    DuplicateSourceFile { file: String },
    /// Explicit files selected for one package did not share one directory.
    SourceFilesFromDifferentDirectories {
        expected_directory: String,
        file: String,
        found_directory: String,
    },
    /// An explicit source argument did not identify a regular `.go` file.
    InvalidSourceFile { file: String, reason: String },
    /// A module directive did not contain a canonical import-like path.
    InvalidModulePath {
        module_path: String,
        issue: ImportPathIssue,
    },
    /// A canonical package directory could not form a valid Go import path.
    InvalidPackageImportPath {
        directory: String,
        import_path: String,
        issue: ImportPathIssue,
    },
    /// Canonicalization showed a local import escaping through a symlink.
    LocalImportOutsideModule {
        import_path: String,
        module_root: String,
        resolved_path: String,
    },
    /// Local packages formed an import cycle.
    ImportCycle { cycle: Vec<String> },
}

impl std::fmt::Display for PathParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(message) => formatter.write_str(message),
            Self::ParserError(error) => write!(formatter, "{error}"),
            Self::InvalidImportPath(error) => write!(formatter, "{error}"),
            Self::NoGoFiles(directory) => {
                write!(formatter, "no Go files found in '{directory}'")
            }
            Self::PackageMismatch {
                expected,
                found,
                file,
            } => write!(
                formatter,
                "found packages {found} ({file}) and {expected} in same directory"
            ),
            Self::DuplicateSourceFile { file } => {
                write!(
                    formatter,
                    "source file was supplied more than once: '{file}'"
                )
            }
            Self::SourceFilesFromDifferentDirectories {
                expected_directory,
                file,
                found_directory,
            } => write!(
                formatter,
                "source file '{file}' is in '{found_directory}', expected '{expected_directory}'"
            ),
            Self::InvalidSourceFile { file, reason } => {
                write!(formatter, "invalid source file '{file}': {reason}")
            }
            Self::InvalidModulePath { module_path, issue } => {
                write!(formatter, "invalid module path {module_path:?}: {issue}")
            }
            Self::InvalidPackageImportPath {
                directory,
                import_path,
                issue,
            } => write!(
                formatter,
                "package directory '{directory}' maps to invalid import path {import_path:?}: {issue}"
            ),
            Self::LocalImportOutsideModule {
                import_path,
                module_root,
                resolved_path,
            } => write!(
                formatter,
                "local import {import_path:?} resolves outside module root '{module_root}' to '{resolved_path}'"
            ),
            Self::ImportCycle { cycle } => {
                write!(formatter, "import cycle: {}", cycle.join(" -> "))
            }
        }
    }
}

impl std::error::Error for PathParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ParserError(error) => Some(error),
            Self::InvalidImportPath(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParsedFileError> for PathParseError {
    fn from(error: ParsedFileError) -> Self {
        match error {
            ParsedFileError::Parser(error) => Self::ParserError(error),
            ParsedFileError::InvalidImportPath(error) => Self::InvalidImportPath(error),
        }
    }
}

/// Immutable source inputs for one Go package.
///
/// The package owns no merged syntax tree. Each [`ParsedFile`] can produce an
/// AST borrowing only its own reference-counted source snapshot.
#[derive(Clone, Debug)]
pub struct ParsedPackage {
    name: String,
    import_path: String,
    files: Arc<[ParsedFile]>,
}

impl ParsedPackage {
    /// Package-clause name shared by every file.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Canonical Go import path, or an empty string for the entry package.
    #[must_use]
    pub fn import_path(&self) -> &str {
        &self.import_path
    }

    /// Deterministically ordered, independently parsed source files.
    #[must_use]
    pub fn files(&self) -> &[ParsedFile] {
        &self.files
    }
}

/// Immutable main package, local-package closure, and stdlib import roots.
#[derive(Clone, Debug)]
pub struct ParsedProgram {
    main_package: ParsedPackage,
    imports: Arc<[ParsedPackage]>,
    stdlib_imports: Arc<[String]>,
}

impl ParsedProgram {
    /// Entry package selected by the invocation.
    #[must_use]
    pub fn main_package(&self) -> &ParsedPackage {
        &self.main_package
    }

    /// Recursively resolved local packages in dependency-first order.
    #[must_use]
    pub fn imports(&self) -> &[ParsedPackage] {
        &self.imports
    }

    /// Reachable imports recognized as packages in the pinned Go SDK.
    #[must_use]
    pub fn stdlib_imports(&self) -> &[String] {
        &self.stdlib_imports
    }
}

/// Parse a Go file or directory into independently owned file products.
///
/// Directories include deterministically sorted `.go` files, excluding test,
/// hidden, and underscore-prefixed files. All selected files must declare the
/// same package. No synthetic package-wide AST is constructed.
pub fn parse_path(path: &str) -> std::result::Result<ParsedPackage, PathParseError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| io_error(format!("cannot access '{path}': {error}")))?;
    if metadata.is_file() {
        parse_source_paths(&[path.to_string()], String::new(), path)
    } else if metadata.is_dir() {
        parse_directory(path, String::new())
    } else {
        Err(io_error(format!("'{path}' is not a file or directory")))
    }
}

/// Parse a Go program and recursively discover local-module imports.
pub fn parse_program(path: &str) -> std::result::Result<ParsedProgram, PathParseError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| io_error(format!("cannot access '{path}': {error}")))?;
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| io_error(format!("cannot canonicalize '{path}': {error}")))?;
    let directory = if metadata.is_file() {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| io_error(format!("source file '{path}' has no parent directory")))?
    } else {
        canonical
    };
    let main_package = parse_path(path)?;
    finish_program(main_package, &directory)
}

/// Parse an explicit set of Go files as one package without merging their ASTs.
///
/// A single path retains [`parse_program`] semantics and may therefore name a
/// directory. Multiple paths must each name a source file.
pub fn parse_program_files(
    file_paths: &[String],
) -> std::result::Result<ParsedProgram, PathParseError> {
    let Some(first_path) = file_paths.first() else {
        return Err(PathParseError::NoGoFiles("(no files given)".to_string()));
    };
    if file_paths.len() == 1 {
        return parse_program(first_path);
    }

    let explicit = normalize_explicit_source_paths(file_paths)?;
    let main_package = parse_source_paths(&explicit.paths, String::new(), "(no files given)")?;
    finish_program(main_package, &explicit.directory)
}

struct ExplicitSourcePaths {
    paths: Vec<String>,
    directory: PathBuf,
}

fn normalize_explicit_source_paths(
    file_paths: &[String],
) -> std::result::Result<ExplicitSourcePaths, PathParseError> {
    let mut canonical_paths = Vec::with_capacity(file_paths.len());
    for file in file_paths {
        let canonical =
            std::fs::canonicalize(file).map_err(|error| PathParseError::InvalidSourceFile {
                file: file.clone(),
                reason: format!("cannot canonicalize path: {error}"),
            })?;
        let metadata =
            std::fs::metadata(&canonical).map_err(|error| PathParseError::InvalidSourceFile {
                file: file.clone(),
                reason: format!("cannot read metadata: {error}"),
            })?;
        if !metadata.is_file() {
            return Err(PathParseError::InvalidSourceFile {
                file: file.clone(),
                reason: "expected a regular file".to_string(),
            });
        }
        if canonical
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("go")
        {
            return Err(PathParseError::InvalidSourceFile {
                file: file.clone(),
                reason: "expected a .go source file".to_string(),
            });
        }
        canonical_paths.push(canonical);
    }
    canonical_paths.sort();

    for duplicate in canonical_paths.windows(2) {
        if duplicate[0] == duplicate[1] {
            return Err(PathParseError::DuplicateSourceFile {
                file: display_path(&duplicate[0]),
            });
        }
    }

    let first = canonical_paths
        .first()
        .ok_or_else(|| PathParseError::NoGoFiles("(no files given)".to_string()))?;
    let directory =
        first
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| PathParseError::InvalidSourceFile {
                file: display_path(first),
                reason: "source file has no parent directory".to_string(),
            })?;
    for file in canonical_paths.iter().skip(1) {
        let found_directory = file
            .parent()
            .ok_or_else(|| PathParseError::InvalidSourceFile {
                file: display_path(file),
                reason: "source file has no parent directory".to_string(),
            })?;
        if found_directory != directory {
            return Err(PathParseError::SourceFilesFromDifferentDirectories {
                expected_directory: display_path(&directory),
                file: display_path(file),
                found_directory: display_path(found_directory),
            });
        }
    }

    let paths = canonical_paths
        .into_iter()
        .map(|path| path_to_utf8(path))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ExplicitSourcePaths { paths, directory })
}

/// Parse one in-memory Go source file for filesystem-free environments.
pub fn parse_program_from_source(
    filename: &str,
    source: &str,
) -> std::result::Result<ParsedProgram, PathParseError> {
    let file = ParsedFile::from_source(filename.to_string(), source.to_string())
        .map_err(PathParseError::from)?;
    let main_package = package_from_files(String::new(), vec![file], filename)?;
    let mut stdlib_imports = Vec::new();
    collect_stdlib_imports(&main_package, &mut stdlib_imports);
    Ok(ParsedProgram {
        main_package,
        imports: Arc::from([]),
        stdlib_imports: stdlib_imports.into(),
    })
}

fn finish_program(
    mut main_package: ParsedPackage,
    directory: &Path,
) -> std::result::Result<ParsedProgram, PathParseError> {
    let module_root = find_module_root(directory);
    let module_name = module_root.as_deref().map(parse_go_mod).transpose()?;
    let mut imports = Vec::new();
    let mut stdlib_imports = Vec::new();
    if let (Some(root), Some(name)) = (module_root.as_deref(), module_name.as_deref()) {
        main_package.import_path = module_package_import_path(root, name, directory)?;
        let mut visited = HashSet::from([main_package.import_path.clone()]);
        let mut active = vec![main_package.import_path.clone()];
        resolve_imports_recursive(
            &main_package,
            root,
            name,
            &mut imports,
            &mut stdlib_imports,
            &mut visited,
            &mut active,
        )?;
    } else {
        collect_stdlib_imports(&main_package, &mut stdlib_imports);
    }
    Ok(ParsedProgram {
        main_package,
        imports: imports.into(),
        stdlib_imports: stdlib_imports.into(),
    })
}

fn parse_directory(
    directory: &str,
    import_path: String,
) -> std::result::Result<ParsedPackage, PathParseError> {
    let paths = eligible_go_files(directory)?;
    parse_source_paths(&paths, import_path, directory)
}

fn eligible_go_files(directory: &str) -> std::result::Result<Vec<String>, PathParseError> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| io_error(format!("cannot read directory '{directory}': {error}")))?;
    let mut paths = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let filename = path.file_name()?.to_str()?;
            let eligible = !filename.starts_with('.')
                && !filename.starts_with('_')
                && filename.ends_with(".go")
                && !filename.ends_with("_test.go");
            eligible.then(|| path.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        Err(PathParseError::NoGoFiles(directory.to_string()))
    } else {
        Ok(paths)
    }
}

fn parse_source_paths(
    paths: &[String],
    import_path: String,
    empty_context: &str,
) -> std::result::Result<ParsedPackage, PathParseError> {
    if paths.is_empty() {
        return Err(PathParseError::NoGoFiles(empty_context.to_string()));
    }
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let source = std::fs::read_to_string(path)
            .map_err(|error| io_error(format!("cannot read '{path}': {error}")))?;
        files.push(ParsedFile::from_source(path.clone(), source).map_err(PathParseError::from)?);
    }
    package_from_files(import_path, files, empty_context)
}

fn package_from_files(
    import_path: String,
    mut files: Vec<ParsedFile>,
    empty_context: &str,
) -> std::result::Result<ParsedPackage, PathParseError> {
    files.sort_by(|left, right| left.path().cmp(right.path()));
    let Some(first) = files.first() else {
        return Err(PathParseError::NoGoFiles(empty_context.to_string()));
    };
    let expected = first.package_name().to_string();

    for file in files.iter().skip(1) {
        if file.package_name() != expected {
            return Err(PathParseError::PackageMismatch {
                expected,
                found: file.package_name().to_string(),
                file: file.path().to_string(),
            });
        }
    }
    Ok(ParsedPackage {
        name: expected,
        import_path,
        files: files.into(),
    })
}

fn package_import_paths(package: &ParsedPackage) -> Vec<String> {
    let mut paths = package
        .files()
        .iter()
        .flat_map(|file| file.imports().iter().cloned())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn collect_stdlib_imports(package: &ParsedPackage, stdlib_imports: &mut Vec<String>) {
    for import_path in package_import_paths(package) {
        if crate::resolve::is_known(&import_path) && !stdlib_imports.contains(&import_path) {
            stdlib_imports.push(import_path);
        }
    }
}

fn resolve_imports_recursive(
    package: &ParsedPackage,
    module_root: &Path,
    module_name: &str,
    imports: &mut Vec<ParsedPackage>,
    stdlib_imports: &mut Vec<String>,
    visited: &mut HashSet<String>,
    active: &mut Vec<String>,
) -> std::result::Result<(), PathParseError> {
    for import_path in package_import_paths(package) {
        let Some(relative_path) = module_relative_path(&import_path, module_name) else {
            if crate::resolve::is_known(&import_path) && !stdlib_imports.contains(&import_path) {
                stdlib_imports.push(import_path);
            }
            continue;
        };
        if let Some(cycle_start) = active.iter().position(|package| package == &import_path) {
            let mut cycle = active[cycle_start..].to_vec();
            cycle.push(import_path);
            return Err(PathParseError::ImportCycle { cycle });
        }
        if visited.contains(&import_path) {
            continue;
        }
        let candidate = module_root.join(relative_path);
        let package_directory = std::fs::canonicalize(&candidate).map_err(|error| {
            io_error(format!(
                "cannot find package '{}' at {}: {error}",
                import_path,
                candidate.display()
            ))
        })?;
        if !package_directory.starts_with(module_root) {
            return Err(PathParseError::LocalImportOutsideModule {
                import_path,
                module_root: display_path(module_root),
                resolved_path: display_path(&package_directory),
            });
        }
        let metadata = std::fs::metadata(&package_directory).map_err(|error| {
            io_error(format!(
                "cannot inspect package '{}' at {}: {error}",
                import_path,
                package_directory.display()
            ))
        })?;
        if !metadata.is_dir() {
            return Err(io_error(format!(
                "cannot find package '{}' at {}",
                import_path,
                package_directory.display()
            )));
        }
        visited.insert(import_path.clone());
        let package_directory = path_ref_to_utf8(&package_directory)?;
        let imported = parse_directory(package_directory, import_path.clone())?;
        active.push(import_path);
        let recursive_result = resolve_imports_recursive(
            &imported,
            module_root,
            module_name,
            imports,
            stdlib_imports,
            visited,
            active,
        );
        active.pop();
        recursive_result?;
        imports.push(imported);
    }
    Ok(())
}

fn module_package_import_path(
    module_root: &Path,
    module_name: &str,
    package_directory: &Path,
) -> std::result::Result<String, PathParseError> {
    let relative = package_directory.strip_prefix(module_root).map_err(|_| {
        PathParseError::LocalImportOutsideModule {
            import_path: module_name.to_string(),
            module_root: display_path(module_root),
            resolved_path: display_path(package_directory),
        }
    })?;
    let mut import_path = module_name.to_string();
    for component in relative.components() {
        let Component::Normal(element) = component else {
            return Err(io_error(format!(
                "package directory '{}' is not canonical",
                package_directory.display()
            )));
        };
        let element = element
            .to_str()
            .ok_or_else(|| PathParseError::InvalidPackageImportPath {
                directory: display_path(package_directory),
                import_path: import_path.clone(),
                issue: ImportPathIssue::InvalidUtf8,
            })?;
        import_path.push('/');
        import_path.push_str(element);
    }
    validate(&import_path).map_err(|issue| PathParseError::InvalidPackageImportPath {
        directory: display_path(package_directory),
        import_path: import_path.clone(),
        issue,
    })?;
    Ok(import_path)
}

fn module_relative_path<'path>(import_path: &'path str, module_name: &str) -> Option<&'path str> {
    if import_path == module_name {
        Some("")
    } else {
        import_path
            .strip_prefix(module_name)
            .and_then(|suffix| suffix.strip_prefix('/'))
    }
}

fn find_module_root(start_directory: &Path) -> Option<PathBuf> {
    let mut directory = start_directory.to_path_buf();
    loop {
        if directory.join("go.mod").exists() {
            return Some(directory);
        }
        if !directory.pop() {
            return None;
        }
    }
}

fn parse_go_mod(module_root: &Path) -> std::result::Result<String, PathParseError> {
    let path = module_root.join("go.mod");
    let content = std::fs::read_to_string(&path)
        .map_err(|error| io_error(format!("cannot read go.mod: {error}")))?;
    let module_path = content
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("module ").map(str::trim))
        .ok_or_else(|| io_error("go.mod does not contain a module directive"))?;
    let decoded = if module_path.starts_with(['"', '`']) {
        decode_and_validate(module_path)
    } else {
        validate(module_path).map(|()| module_path.to_string())
    };
    decoded.map_err(|issue| PathParseError::InvalidModulePath {
        module_path: module_path.to_string(),
        issue,
    })
}

fn io_error(message: impl Into<String>) -> PathParseError {
    PathParseError::IoError(message.into())
}

fn path_to_utf8(path: PathBuf) -> std::result::Result<String, PathParseError> {
    path.into_os_string()
        .into_string()
        .map_err(|path| PathParseError::InvalidSourceFile {
            file: path.to_string_lossy().into_owned(),
            reason: "source path is not valid UTF-8".to_string(),
        })
}

fn path_ref_to_utf8(path: &Path) -> std::result::Result<&str, PathParseError> {
    path.to_str()
        .ok_or_else(|| PathParseError::InvalidSourceFile {
            file: display_path(path),
            reason: "source path is not valid UTF-8".to_string(),
        })
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests;
