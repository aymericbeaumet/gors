use crate::import_path::CanonicalImportPath;

use super::ModuleFileIssue;

/// Read the one canonical module identity from UTF-8 `go.mod` text.
///
/// This deliberately recognizes only the line-oriented `module` directive. It
/// ignores every other directive, performs no dependency resolution, and
/// rejects blocks, extra operands, escaped quoted paths, and duplicates.
pub fn parse_module_directive(source: &str) -> Result<CanonicalImportPath, ModuleFileIssue> {
    let mut module = None;
    for (line_index, raw_line) in source.lines().enumerate() {
        let line_number = line_index.saturating_add(1);
        let line = strip_line_comment(raw_line).trim();
        let mut fields = line.split_whitespace();
        if fields.next() != Some("module") {
            continue;
        }
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
        let path =
            CanonicalImportPath::new(path).map_err(|issue| ModuleFileIssue::InvalidModulePath {
                line: line_number,
                issue,
            })?;
        module = Some((path, line_number));
    }

    module
        .map(|(path, _)| path)
        .ok_or(ModuleFileIssue::MissingModuleDirective)
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
