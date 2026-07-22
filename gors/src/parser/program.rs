use super::{ParserError, ast, parse_file};

/// Parse a Go source path (file or directory) into an Abstract Syntax Tree.
///
/// This function handles both individual Go files and directories:
/// - For a file path: parses that single file
/// - For a directory path: parses all `.go` files in the directory (excluding `_test.go` files)
///   and merges them into a single AST
///
/// This matches the behavior of `go run` and `go build`.
///
/// # Arguments
///
/// * `path` - Path to a Go source file or directory containing Go files
///
/// # Returns
///
/// Returns `Ok(ast::File)` on successful parsing, or `Err(ParserError)`
/// if the source contains syntax errors or no Go files are found.
///
/// # Example
///
/// ```no_run
/// use gors::parser::parse_path;
///
/// // Parse a single file
/// let ast = parse_path("main.go").unwrap();
///
/// // Parse all Go files in a directory
/// let ast = parse_path("./mypackage/").unwrap();
/// ```
pub fn parse_path(
    path: &str,
) -> std::result::Result<(ast::File<'static>, Vec<(String, String)>), PathParseError> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| PathParseError::IoError(format!("cannot access '{}': {}", path, e)))?;

    if metadata.is_file() {
        let buffer = std::fs::read_to_string(path)
            .map_err(|e| PathParseError::IoError(format!("cannot read '{}': {}", path, e)))?;

        // We need to leak the strings to get 'static lifetime
        let path_static: &'static str = Box::leak(path.to_string().into_boxed_str());
        let buffer_static: &'static str = Box::leak(buffer.clone().into_boxed_str());

        let ast = parse_file(path_static, buffer_static).map_err(PathParseError::ParserError)?;

        Ok((ast, vec![(path.to_string(), buffer)]))
    } else if metadata.is_dir() {
        parse_dir(path)
    } else {
        Err(PathParseError::IoError(format!(
            "'{}' is not a file or directory",
            path
        )))
    }
}

/// Error type for path parsing failures.
#[derive(Debug)]
pub enum PathParseError {
    /// An I/O error occurred (file not found, permission denied, etc.)
    IoError(String),
    /// A parser error occurred while parsing a Go file
    ParserError(ParserError),
    /// No Go files found in the directory
    NoGoFiles(String),
    /// Package name mismatch between files
    PackageMismatch {
        expected: String,
        found: String,
        file: String,
    },
}

impl std::fmt::Display for PathParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "{}", msg),
            Self::ParserError(e) => write!(f, "{}", e),
            Self::NoGoFiles(dir) => write!(f, "no Go files found in '{}'", dir),
            Self::PackageMismatch {
                expected,
                found,
                file,
            } => {
                write!(
                    f,
                    "found packages {} ({}) and {} in same directory",
                    found, file, expected
                )
            }
        }
    }
}

impl std::error::Error for PathParseError {}

/// Parse all Go files in a directory into a single merged AST.
///
/// This function reads all `.go` files in the specified directory (excluding
/// `_test.go` files and files starting with `.` or `_`), parses them, and
/// merges their declarations into a single AST.
///
/// All files must declare the same package name.
///
/// # Arguments
///
/// * `dir_path` - Path to a directory containing Go source files
///
/// # Returns
///
/// Returns a tuple of the merged AST and the list of (filename, content) pairs
/// for all parsed files.
fn parse_dir(
    dir_path: &str,
) -> std::result::Result<(ast::File<'static>, Vec<(String, String)>), PathParseError> {
    let entries = std::fs::read_dir(dir_path).map_err(|e| {
        PathParseError::IoError(format!("cannot read directory '{}': {}", dir_path, e))
    })?;

    // Collect all .go files (excluding _test.go and dotfiles)
    let mut go_files: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;

            // Skip hidden files, underscore-prefixed files, and test files
            if file_name.starts_with('.') || file_name.starts_with('_') {
                return None;
            }

            // Only include .go files, excluding _test.go
            if file_name.ends_with(".go") && !file_name.ends_with("_test.go") {
                Some(path.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();

    if go_files.is_empty() {
        return Err(PathParseError::NoGoFiles(dir_path.to_string()));
    }

    // Sort for deterministic ordering
    go_files.sort();

    // Parse all files and collect their ASTs
    let mut files_content: Vec<(String, String)> = Vec::new();
    let mut asts: Vec<ast::File<'static>> = Vec::new();

    for file_path in &go_files {
        let buffer = std::fs::read_to_string(file_path)
            .map_err(|e| PathParseError::IoError(format!("cannot read '{}': {}", file_path, e)))?;

        // Leak strings to get 'static lifetime
        let path_static: &'static str = Box::leak(file_path.clone().into_boxed_str());
        let buffer_static: &'static str = Box::leak(buffer.clone().into_boxed_str());

        let ast = parse_file(path_static, buffer_static).map_err(PathParseError::ParserError)?;

        files_content.push((file_path.clone(), buffer));
        asts.push(ast);
    }

    // Verify all files have the same package name
    let Some(expected_package) = asts.first().map(|ast| ast.name.name) else {
        return Err(PathParseError::NoGoFiles(dir_path.to_string()));
    };
    for (i, ast) in asts.iter().enumerate().skip(1) {
        if ast.name.name != expected_package {
            return Err(PathParseError::PackageMismatch {
                expected: expected_package.to_string(),
                found: ast.name.name.to_string(),
                file: go_files
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "(unknown file)".to_string()),
            });
        }
    }

    // Merge all ASTs into one
    let merged = merge_files(asts);

    Ok((merged, files_content))
}

/// Merge multiple Go AST files into a single file.
///
/// This combines all declarations from the input files into a single AST,
/// using the package information from the first file.
fn merge_files(mut files: Vec<ast::File<'static>>) -> ast::File<'static> {
    if files.len() == 1 {
        return files.remove(0);
    }

    let mut base = files.remove(0);

    for file in files {
        // Merge declarations
        base.decls.extend(file.decls);

        // Merge unresolved identifiers
        base.unresolved.extend(file.unresolved);

        // Merge comments
        base.comments.extend(file.comments);

        // Update file_end to the last file's end
        base.file_end = file.file_end;
    }

    base
}

/// A parsed package with its AST and metadata.
#[derive(Debug)]
pub struct ParsedPackage {
    pub name: String,
    pub import_path: String,
    pub ast: ast::File<'static>,
    pub files: Vec<(String, String)>,
}

/// A fully resolved program: main package plus all local imports.
#[derive(Debug)]
pub struct ParsedProgram {
    pub main_package: ParsedPackage,
    pub imports: Vec<ParsedPackage>,
    pub stdlib_imports: Vec<String>,
}

/// Parse a Go program with full import resolution.
///
/// If the directory contains a go.mod, local imports are resolved recursively.
/// Standard library imports (e.g., "fmt") are skipped.
pub fn parse_program(path: &str) -> std::result::Result<ParsedProgram, PathParseError> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| PathParseError::IoError(format!("cannot access '{}': {}", path, e)))?;

    let dir_path = if metadata.is_file() {
        std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string())
    } else {
        path.to_string()
    };

    let (main_ast, files) = parse_path(path)?;

    let module_root = find_module_root(&dir_path);
    let module_name = module_root
        .as_ref()
        .and_then(|root| parse_go_mod(root).ok());

    let mut imports = Vec::new();
    let mut stdlib_imports = Vec::new();
    if let (Some(root), Some(mod_name)) = (&module_root, &module_name) {
        let mut visited = std::collections::HashSet::new();
        resolve_imports_recursive(
            &main_ast,
            root,
            mod_name,
            &mut imports,
            &mut stdlib_imports,
            &mut visited,
        )?;
    } else {
        collect_stdlib_imports(&main_ast, &mut stdlib_imports);
    }

    let pkg_name = main_ast.name.name.to_string();
    Ok(ParsedProgram {
        main_package: ParsedPackage {
            name: pkg_name,
            import_path: String::new(),
            ast: main_ast,
            files,
        },
        imports,
        stdlib_imports,
    })
}

/// Parse a Go program from an explicit list of source file paths.
///
/// All files must be `.go` files in the same package. If only one path is given,
/// delegates to [`parse_program`]. For multiple files, parses each individually
/// and merges their ASTs before resolving imports.
pub fn parse_program_files(
    file_paths: &[String],
) -> std::result::Result<ParsedProgram, PathParseError> {
    if file_paths.is_empty() {
        return Err(PathParseError::NoGoFiles("(no files given)".to_string()));
    }
    if file_paths.len() == 1 {
        return parse_program(
            file_paths
                .first()
                .map(String::as_str)
                .unwrap_or("(no files given)"),
        );
    }

    let (main_ast, files) = parse_explicit_files(file_paths)?;

    let dir_path = std::path::Path::new(
        file_paths
            .first()
            .map(String::as_str)
            .unwrap_or("(no files given)"),
    )
    .parent()
    .map(|p| p.to_string_lossy().into_owned())
    .unwrap_or_else(|| ".".to_string());

    let module_root = find_module_root(&dir_path);
    let module_name = module_root
        .as_ref()
        .and_then(|root| parse_go_mod(root).ok());

    let mut imports = Vec::new();
    let mut stdlib_imports = Vec::new();
    if let (Some(root), Some(mod_name)) = (&module_root, &module_name) {
        let mut visited = std::collections::HashSet::new();
        resolve_imports_recursive(
            &main_ast,
            root,
            mod_name,
            &mut imports,
            &mut stdlib_imports,
            &mut visited,
        )?;
    } else {
        collect_stdlib_imports(&main_ast, &mut stdlib_imports);
    }

    let pkg_name = main_ast.name.name.to_string();
    Ok(ParsedProgram {
        main_package: ParsedPackage {
            name: pkg_name,
            import_path: String::new(),
            ast: main_ast,
            files,
        },
        imports,
        stdlib_imports,
    })
}

/// Parse a Go program from in-memory source code.
///
/// This is the entry point for environments without filesystem access (e.g. WASM).
/// It parses the source, extracts stdlib imports, and returns a `ParsedProgram`
/// identical to what [`parse_program`] would return for a single-file program.
pub fn parse_program_from_source(
    filename: &str,
    source: &str,
) -> std::result::Result<ParsedProgram, PathParseError> {
    let filename_static: &'static str = Box::leak(filename.to_string().into_boxed_str());
    let source_static: &'static str = Box::leak(source.to_string().into_boxed_str());
    let ast = parse_file(filename_static, source_static).map_err(PathParseError::ParserError)?;

    let mut stdlib_imports = Vec::new();
    collect_stdlib_imports(&ast, &mut stdlib_imports);

    let pkg_name = ast.name.name.to_string();
    Ok(ParsedProgram {
        main_package: ParsedPackage {
            name: pkg_name,
            import_path: String::new(),
            ast,
            files: vec![(filename.to_string(), source.to_string())],
        },
        imports: vec![],
        stdlib_imports,
    })
}

fn parse_explicit_files(
    file_paths: &[String],
) -> std::result::Result<(ast::File<'static>, Vec<(String, String)>), PathParseError> {
    if file_paths.is_empty() {
        return Err(PathParseError::NoGoFiles("(no files given)".to_string()));
    }

    let mut files_content: Vec<(String, String)> = Vec::new();
    let mut asts: Vec<ast::File<'static>> = Vec::new();

    for file_path in file_paths {
        let buffer = std::fs::read_to_string(file_path)
            .map_err(|e| PathParseError::IoError(format!("cannot read '{}': {}", file_path, e)))?;
        let path_static: &'static str = Box::leak(file_path.clone().into_boxed_str());
        let buffer_static: &'static str = Box::leak(buffer.clone().into_boxed_str());
        let ast = parse_file(path_static, buffer_static).map_err(PathParseError::ParserError)?;
        files_content.push((file_path.clone(), buffer));
        asts.push(ast);
    }

    let Some(expected_package) = asts.first().map(|ast| ast.name.name) else {
        return Err(PathParseError::NoGoFiles("(no files given)".to_string()));
    };
    for (i, ast) in asts.iter().enumerate().skip(1) {
        if ast.name.name != expected_package {
            return Err(PathParseError::PackageMismatch {
                expected: expected_package.to_string(),
                found: ast.name.name.to_string(),
                file: file_paths
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "(unknown file)".to_string()),
            });
        }
    }

    let merged = merge_files(asts);
    Ok((merged, files_content))
}

fn find_module_root(start_dir: &str) -> Option<String> {
    let mut dir = std::path::PathBuf::from(start_dir);
    loop {
        if dir.join("go.mod").exists() {
            return Some(dir.to_string_lossy().into_owned());
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn parse_go_mod(module_root: &str) -> std::result::Result<String, PathParseError> {
    let go_mod_path = std::path::Path::new(module_root).join("go.mod");
    let content = std::fs::read_to_string(&go_mod_path)
        .map_err(|e| PathParseError::IoError(format!("cannot read go.mod: {}", e)))?;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("module ") {
            return Ok(rest.trim().to_string());
        }
    }

    Err(PathParseError::IoError(
        "go.mod does not contain a module directive".to_string(),
    ))
}

fn collect_stdlib_imports(file: &ast::File<'_>, stdlib_imports: &mut Vec<String>) {
    for import_spec in file.imports() {
        let import_path = import_spec.path.value.trim_matches('"');
        if crate::resolve::is_known(import_path)
            && !stdlib_imports.contains(&import_path.to_string())
        {
            stdlib_imports.push(import_path.to_string());
        }
    }
}

fn resolve_imports_recursive(
    file: &ast::File<'static>,
    module_root: &str,
    module_name: &str,
    imports: &mut Vec<ParsedPackage>,
    stdlib_imports: &mut Vec<String>,
    visited: &mut std::collections::HashSet<String>,
) -> std::result::Result<(), PathParseError> {
    for import_spec in file.imports() {
        let raw_path = import_spec.path.value;
        let import_path = raw_path.trim_matches('"');

        if visited.contains(import_path) {
            continue;
        }

        let rel_path = match import_path.strip_prefix(module_name) {
            Some(rest) => rest.trim_start_matches('/'),
            None => {
                if crate::resolve::is_known(import_path)
                    && !stdlib_imports.contains(&import_path.to_string())
                {
                    stdlib_imports.push(import_path.to_string());
                }
                continue;
            }
        };

        let pkg_dir = std::path::Path::new(module_root).join(rel_path);
        if !pkg_dir.is_dir() {
            return Err(PathParseError::IoError(format!(
                "cannot find package '{}' at {}",
                import_path,
                pkg_dir.display()
            )));
        }

        visited.insert(import_path.to_string());

        let pkg_dir_str = pkg_dir.to_string_lossy().into_owned();
        let (pkg_ast, pkg_files) = parse_dir(&pkg_dir_str)?;

        resolve_imports_recursive(
            &pkg_ast,
            module_root,
            module_name,
            imports,
            stdlib_imports,
            visited,
        )?;

        let pkg_name = pkg_ast.name.name.to_string();
        imports.push(ParsedPackage {
            name: pkg_name,
            import_path: import_path.to_string(),
            ast: pkg_ast,
            files: pkg_files,
        });
    }

    Ok(())
}
