use std::sync::Arc;

use crate::compiler::input::GoLanguageVersion;
use crate::import_path::CanonicalImportPath;

use super::ModuleFileIssue;

/// Parsed source-selection metadata from one local `go.mod` file.
pub(super) struct ModuleMetadata {
    pub(super) module_path: CanonicalImportPath,
    pub(super) language_version: GoLanguageVersion,
}

/// Read the one canonical module identity from UTF-8 `go.mod` text.
///
/// Parsing also validates the optional line-oriented `go` directive because
/// both values form the module's source-selection metadata. Every other
/// directive is ignored; this performs no dependency resolution and rejects
/// blocks, extra operands, escaped quoted paths, and duplicates.
pub fn parse_module_directive(source: &str) -> Result<CanonicalImportPath, ModuleFileIssue> {
    parse_module_metadata(source).map(|metadata| metadata.module_path)
}

/// Read the language version selected by a local `go.mod` file.
///
/// Go assigns the fixed `go1.16` compatibility version when the directive is
/// absent; this default deliberately does not track the current toolchain.
pub fn parse_language_version_directive(
    source: &str,
) -> Result<GoLanguageVersion, ModuleFileIssue> {
    parse_module_metadata(source).map(|metadata| metadata.language_version)
}

pub(super) fn parse_module_metadata(source: &str) -> Result<ModuleMetadata, ModuleFileIssue> {
    let mut module = None;
    let mut language_version = None;
    for (line_index, raw_line) in source.lines().enumerate() {
        let line_number = line_index.saturating_add(1);
        let line = strip_line_comment(raw_line).trim();
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("module") => {
                if let Some((_, first_line)) = &module {
                    return Err(ModuleFileIssue::DuplicateModuleDirective {
                        first_line: *first_line,
                        duplicate_line: line_number,
                    });
                }
                let Some(operand) = fields.next() else {
                    return Err(ModuleFileIssue::MalformedModuleDirective { line: line_number });
                };
                if fields.next().is_some() {
                    return Err(ModuleFileIssue::MalformedModuleDirective { line: line_number });
                }
                let path = canonical_operand(operand)
                    .ok_or(ModuleFileIssue::MalformedModuleDirective { line: line_number })?;
                let path = CanonicalImportPath::new(path).map_err(|issue| {
                    ModuleFileIssue::InvalidModulePath {
                        line: line_number,
                        issue,
                    }
                })?;
                module = Some((path, line_number));
            }
            Some("go") => {
                if let Some((_, first_line)) = language_version {
                    return Err(ModuleFileIssue::DuplicateGoDirective {
                        first_line,
                        duplicate_line: line_number,
                    });
                }
                let Some(operand) = fields.next() else {
                    return Err(ModuleFileIssue::MalformedGoDirective { line: line_number });
                };
                if fields.next().is_some() {
                    return Err(ModuleFileIssue::MalformedGoDirective { line: line_number });
                }
                let version = GoLanguageVersion::parse(operand).map_err(|issue| {
                    ModuleFileIssue::InvalidGoVersion {
                        line: line_number,
                        version: Arc::from(operand),
                        issue,
                    }
                })?;
                language_version = Some((version, line_number));
            }
            _ => {}
        }
    }

    let module_path = module
        .map(|(path, _)| path)
        .ok_or(ModuleFileIssue::MissingModuleDirective)?;
    Ok(ModuleMetadata {
        module_path,
        language_version: language_version
            .map_or(GoLanguageVersion::DEFAULT_MODULE, |(version, _)| version),
    })
}

fn strip_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(prefix, _)| prefix)
}

fn canonical_operand(operand: &str) -> Option<&str> {
    let bytes = operand.as_bytes();
    match bytes {
        [b'"', contents @ .., b'"'] | [b'`', contents @ .., b'`']
            if !contents
                .iter()
                .any(|byte| matches!(byte, b'"' | b'`' | b'\\' | b'\r' | b'\n')) =>
        {
            std::str::from_utf8(contents).ok()
        }
        [b'"' | b'`', ..] => None,
        _ => Some(operand),
    }
}
