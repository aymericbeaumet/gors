use crate::common::{fixtures_dir, workspace_root};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceReport {
    schema_version: u32,
    kind: String,
    title: String,
    source: ReportSource,
    summary: ReportSummary,
    groups: Vec<ReportGroup>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportSource {
    title: String,
    url: String,
    language_version: String,
    published: String,
    retrieved: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportSummary {
    group_count: usize,
    passing_group_count: usize,
    case_count: usize,
    passing_case_count: usize,
    unsupported_case_count: usize,
    fixture_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportGroup {
    id: String,
    title: String,
    subtitle: String,
    fixtures: Vec<String>,
    summary: ReportSummary,
    cases: Vec<ReportCase>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportCase {
    id: String,
    title: String,
    subtitle: String,
    kind: String,
    status: ReportStatus,
    fixtures: Vec<String>,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReportStatus {
    Passing,
    Unsupported,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpecManifest {
    source: SpecSource,
    categories: Vec<SpecCategory>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpecSource {
    title: String,
    url: String,
    language_version: String,
    published: String,
    retrieved: String,
}

#[derive(Deserialize)]
struct SpecCategory {
    name: String,
    tests: Vec<SpecCase>,
}

#[derive(Deserialize)]
struct SpecCase {
    id: String,
    section: String,
    title: String,
    status: String,
    fixtures: Option<Vec<String>>,
    reason: Option<String>,
}

#[derive(Debug)]
struct StdlibSymbol {
    name: String,
    kind: String,
    fixtures: BTreeSet<String>,
}

pub fn write_go_spec_conformance() -> Result<(), String> {
    let manifest_path = fixtures_dir().join("go_spec/spec.json");
    let manifest: SpecManifest = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?,
    )
    .map_err(|e| format!("cannot parse {}: {e}", manifest_path.display()))?;

    let groups = manifest
        .categories
        .into_iter()
        .map(|category| {
            let cases = category
                .tests
                .into_iter()
                .map(|case| {
                    let status = match case.status.as_str() {
                        "passing" => ReportStatus::Passing,
                        _ => ReportStatus::Unsupported,
                    };
                    ReportCase {
                        id: case.id,
                        title: case.title,
                        subtitle: case.section,
                        kind: "spec-test".to_string(),
                        status,
                        fixtures: case.fixtures.unwrap_or_default(),
                        reason: case.reason.unwrap_or_default(),
                    }
                })
                .collect::<Vec<_>>();
            let summary = summarize_cases(&cases, 0);
            ReportGroup {
                id: slug(&category.name),
                title: category.name,
                subtitle: String::new(),
                fixtures: collect_case_fixtures(&cases),
                summary,
                cases,
            }
        })
        .collect::<Vec<_>>();

    let report = ConformanceReport {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: "go-spec".to_string(),
        title: "Go Language Specification Conformance".to_string(),
        source: ReportSource {
            title: manifest.source.title,
            url: manifest.source.url,
            language_version: manifest.source.language_version,
            published: manifest.source.published,
            retrieved: manifest.source.retrieved,
        },
        summary: summarize_groups(&groups),
        groups,
    };
    write_report("go-spec-conformance.json", &report)
}

pub fn write_go_stdlib_conformance(
    passed_fixture_names: &[String],
    attempted_fixture_names: &[String],
    retain_unattempted_fixture_names: bool,
) -> Result<(), String> {
    let fixture_root = fixtures_dir().join("go_stdlib");
    let mut symbols_by_package = collect_stdlib_symbols()?;
    let passed_fixture_names = if retain_unattempted_fixture_names {
        merge_existing_passing_fixture_names(
            "go-stdlib-conformance.json",
            passed_fixture_names,
            attempted_fixture_names,
        )?
    } else {
        passed_fixture_names
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let fixture_names = add_fixture_usage(
        &fixture_root,
        &passed_fixture_names,
        &mut symbols_by_package,
    )?;

    let groups = symbols_by_package
        .into_iter()
        .map(|(package_path, symbols)| {
            let cases = symbols
                .into_values()
                .map(|symbol| {
                    let fixtures = symbol.fixtures.into_iter().collect::<Vec<_>>();
                    let status = if fixtures.is_empty() {
                        ReportStatus::Unsupported
                    } else {
                        ReportStatus::Passing
                    };
                    ReportCase {
                        id: format!("{package_path}::{}", symbol.name),
                        title: symbol.name,
                        subtitle: symbol.kind.clone(),
                        kind: symbol.kind,
                        status,
                        fixtures,
                        reason: String::new(),
                    }
                })
                .collect::<Vec<_>>();
            let summary = summarize_cases(&cases, 0);
            ReportGroup {
                id: package_path.clone(),
                title: package_path,
                subtitle: String::new(),
                fixtures: collect_case_fixtures(&cases),
                summary,
                cases,
            }
        })
        .collect::<Vec<_>>();

    let mut summary = summarize_groups(&groups);
    summary.fixture_count = if retain_unattempted_fixture_names {
        merge_existing_fixture_count(
            "go-stdlib-conformance.json",
            &fixture_names,
            attempted_fixture_names,
        )?
    } else {
        fixture_names.len()
    };
    let report = ConformanceReport {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: "go-stdlib".to_string(),
        title: "Go Standard Library Conformance".to_string(),
        source: ReportSource {
            title: "The Go Standard Library".to_string(),
            url: format!("https://pkg.go.dev/std@go{}", gors::GO_VERSION),
            language_version: format!("go{}", gors::GO_VERSION),
            published: String::new(),
            retrieved: String::new(),
        },
        summary,
        groups,
    };
    write_report("go-stdlib-conformance.json", &report)
}

fn summarize_cases(cases: &[ReportCase], fixture_count: usize) -> ReportSummary {
    let passing_case_count = cases
        .iter()
        .filter(|case| case.status == ReportStatus::Passing)
        .count();
    ReportSummary {
        group_count: 0,
        passing_group_count: 0,
        case_count: cases.len(),
        passing_case_count,
        unsupported_case_count: cases.len() - passing_case_count,
        fixture_count,
    }
}

fn collect_case_fixtures(cases: &[ReportCase]) -> Vec<String> {
    cases
        .iter()
        .flat_map(|case| case.fixtures.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn summarize_groups(groups: &[ReportGroup]) -> ReportSummary {
    let case_count = groups
        .iter()
        .map(|group| group.summary.case_count)
        .sum::<usize>();
    let passing_case_count = groups
        .iter()
        .map(|group| group.summary.passing_case_count)
        .sum::<usize>();
    let passing_group_count = groups
        .iter()
        .filter(|group| group.summary.unsupported_case_count == 0)
        .count();
    ReportSummary {
        group_count: groups.len(),
        passing_group_count,
        case_count,
        passing_case_count,
        unsupported_case_count: case_count - passing_case_count,
        fixture_count: 0,
    }
}

fn report_path(filename: &str) -> PathBuf {
    let report_dir = workspace_root().join("gors/tests/reports");
    report_dir.join(filename)
}

fn write_report(filename: &str, report: &ConformanceReport) -> Result<(), String> {
    let path = report_path(filename);
    let report_dir = path
        .parent()
        .ok_or_else(|| format!("report path has no parent: {}", path.display()))?;
    fs::create_dir_all(report_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    fs::write(&path, format!("{json}\n")).map_err(|e| e.to_string())?;
    eprintln!("Wrote {}", path.display());
    Ok(())
}

fn merge_existing_passing_fixture_names(
    filename: &str,
    passed_fixture_names: &[String],
    attempted_fixture_names: &[String],
) -> Result<Vec<String>, String> {
    let mut fixtures = passed_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let attempted = attempted_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let path = report_path(filename);
    if !path.exists() {
        return Ok(fixtures.into_iter().collect());
    }
    let existing: ConformanceReport = serde_json::from_str(
        &fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    for case in existing.groups.into_iter().flat_map(|group| group.cases) {
        if case.status != ReportStatus::Passing {
            continue;
        }
        for fixture in case.fixtures {
            if !attempted.contains(&fixture) {
                fixtures.insert(fixture);
            }
        }
    }
    Ok(fixtures.into_iter().collect())
}

fn merge_existing_fixture_count(
    filename: &str,
    passed_fixture_names: &[String],
    attempted_fixture_names: &[String],
) -> Result<usize, String> {
    let Some((existing_count, existing_fixtures)) = existing_report_fixture_snapshot(filename)?
    else {
        return Ok(passed_fixture_names.len());
    };
    let passed = passed_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let attempted = attempted_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let added = passed
        .iter()
        .filter(|fixture| attempted.contains(*fixture) && !existing_fixtures.contains(*fixture))
        .count();
    let removed = attempted
        .iter()
        .filter(|fixture| !passed.contains(*fixture) && existing_fixtures.contains(*fixture))
        .count();
    Ok(existing_count.saturating_add(added).saturating_sub(removed))
}

fn existing_report_fixture_snapshot(
    filename: &str,
) -> Result<Option<(usize, BTreeSet<String>)>, String> {
    let path = report_path(filename);
    if !path.exists() {
        return Ok(None);
    }
    let existing: ConformanceReport = serde_json::from_str(
        &fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    let fixtures = existing
        .groups
        .iter()
        .flat_map(|group| group.fixtures.iter().cloned())
        .collect::<BTreeSet<_>>();
    Ok(Some((existing.summary.fixture_count, fixtures)))
}

fn collect_stdlib_symbols() -> Result<BTreeMap<String, BTreeMap<String, StdlibSymbol>>, String> {
    let mut symbols_by_package = BTreeMap::new();
    let src_root = Path::new(gors::GO_SDK_PATH).join("src");
    let mut files = Vec::new();
    collect_go_source_files(&src_root, &mut files)?;
    for file in files {
        let package_path = file
            .parent()
            .and_then(|parent| parent.strip_prefix(&src_root).ok())
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if package_path.is_empty() {
            continue;
        }
        let source = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        if !should_compile_file(
            file.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
            &source,
        ) {
            continue;
        }
        for (name, kind) in parse_exported_symbols(&source) {
            add_symbol(&mut symbols_by_package, &package_path, &name, &kind);
        }
    }
    for builtin in [
        "any", "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len",
        "make", "max", "min", "new", "panic", "print", "println", "real", "recover",
    ] {
        add_symbol(&mut symbols_by_package, "builtin", builtin, "builtin");
    }
    Ok(symbols_by_package)
}

fn collect_go_source_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !matches!(name, "testdata" | "vendor" | "cmd") && !name.starts_with('.') {
                collect_go_source_files(&path, files)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "go")
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_test.go"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn add_fixture_usage(
    fixture_root: &Path,
    passed_fixture_names: &[String],
    symbols_by_package: &mut BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
) -> Result<Vec<String>, String> {
    let fixtures = passed_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for fixture in &fixtures {
        let source_path = fixture_root.join(fixture).join("main.go");
        let source = fs::read_to_string(&source_path)
            .map_err(|e| format!("cannot read {}: {e}", source_path.display()))?;
        add_behavioral_fixture_usage(symbols_by_package, &source, fixture)?;
    }
    Ok(fixtures)
}

fn add_symbol(
    symbols_by_package: &mut BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
    package_path: &str,
    name: &str,
    kind: &str,
) {
    let symbols = symbols_by_package
        .entry(package_path.to_string())
        .or_default();
    symbols
        .entry(name.to_string())
        .and_modify(|symbol| {
            if symbol.kind == "usage" && kind != "usage" {
                symbol.kind = kind.to_string();
            }
        })
        .or_insert_with(|| StdlibSymbol {
            name: name.to_string(),
            kind: kind.to_string(),
            fixtures: BTreeSet::new(),
        });
}

fn mark_tested(
    symbols_by_package: &mut BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
    package_path: &str,
    symbol_name: &str,
    fixture: &str,
) -> Result<(), String> {
    if let Some(symbol) = symbols_by_package
        .get_mut(package_path)
        .and_then(|symbols| symbols.get_mut(symbol_name))
    {
        symbol.fixtures.insert(fixture.to_string());
        Ok(())
    } else {
        Err(format!(
            "{fixture}: behavioral coverage marker references unknown stdlib symbol {package_path}::{symbol_name}"
        ))
    }
}

fn add_behavioral_fixture_usage(
    symbols_by_package: &mut BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
    source: &str,
    fixture: &str,
) -> Result<(), String> {
    for (line_index, line) in source.lines().enumerate() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("// gors:stdlib-cover ") else {
            continue;
        };
        for id in rest.split_whitespace() {
            let Some((package_path, symbol_name)) = id.split_once("::") else {
                return Err(format!(
                    "{fixture}: malformed stdlib coverage marker on line {}: {id}",
                    line_index + 1
                ));
            };
            mark_tested(symbols_by_package, package_path, symbol_name, fixture)?;
        }
    }
    Ok(())
}

fn parse_exported_symbols(source: &str) -> Vec<(String, String)> {
    let source = strip_go_comments(source);
    let mut result = Vec::new();
    let mut group_kind: Option<&str> = None;
    let mut brace_depth = 0usize;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let at_top_level = brace_depth == 0;
        if let Some(kind) = group_kind {
            if line.starts_with(')') {
                group_kind = None;
                brace_depth = update_go_brace_depth(brace_depth, line);
                continue;
            }
            if let Some((name, _)) = read_identifier(line)
                && is_exported(&name)
            {
                result.push((name, kind.to_string()));
            }
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if !at_top_level {
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if matches!(line, "const (" | "var (") {
            group_kind = line.split_whitespace().next();
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if let Some(rest) = line.strip_prefix("func ") {
            if let Some(after_receiver) = rest.strip_prefix('(')
                && let Some(end_receiver) = after_receiver.find(')')
            {
                let receiver = receiver_type_name(&after_receiver[..end_receiver]);
                let after = after_receiver[end_receiver + 1..].trim_start();
                if let Some((name, _)) = read_identifier(after)
                    && is_exported(&receiver)
                    && is_exported(&name)
                {
                    result.push((format!("{receiver}.{name}"), "method".to_string()));
                }
                brace_depth = update_go_brace_depth(brace_depth, line);
                continue;
            }
            if let Some((name, _)) = read_identifier(rest)
                && is_exported(&name)
            {
                result.push((name, "func".to_string()));
            }
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        for (prefix, kind) in [("type ", "type"), ("const ", "const"), ("var ", "var")] {
            if let Some(rest) = line.strip_prefix(prefix)
                && let Some((name, _)) = read_identifier(rest)
                && is_exported(&name)
            {
                result.push((name, kind.to_string()));
            }
        }
        brace_depth = update_go_brace_depth(brace_depth, line);
    }
    result
}

fn update_go_brace_depth(mut depth: usize, line: &str) -> usize {
    let mut chars = line.chars();
    let mut in_string: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            if ch == '\\' && quote != '`' {
                chars.next();
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

fn receiver_type_name(receiver: &str) -> String {
    receiver
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_start_matches('*')
        .split('[')
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_string()
}

fn read_identifier(source: &str) -> Option<(String, usize)> {
    let mut chars = source.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    Some((source[..end].to_string(), end))
}

fn is_exported(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn strip_go_comments(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut state = CommentState::Code;
    while let Some(ch) = chars.next() {
        match state {
            CommentState::Code => match (ch, chars.peek().copied()) {
                ('/', Some('/')) => {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Line;
                }
                ('/', Some('*')) => {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Block;
                }
                ('"', _) => {
                    result.push(ch);
                    state = CommentState::DoubleQuote;
                }
                ('\'', _) => {
                    result.push(ch);
                    state = CommentState::SingleQuote;
                }
                ('`', _) => {
                    result.push(ch);
                    state = CommentState::RawString;
                }
                _ => result.push(ch),
            },
            CommentState::Line => {
                if ch == '\n' {
                    result.push('\n');
                    state = CommentState::Code;
                } else {
                    result.push(' ');
                }
            }
            CommentState::Block => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Code;
                } else {
                    result.push(if ch == '\n' { '\n' } else { ' ' });
                }
            }
            CommentState::DoubleQuote => {
                result.push(ch);
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                } else if ch == '"' {
                    state = CommentState::Code;
                }
            }
            CommentState::SingleQuote => {
                result.push(ch);
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                } else if ch == '\'' {
                    state = CommentState::Code;
                }
            }
            CommentState::RawString => {
                result.push(ch);
                if ch == '`' {
                    state = CommentState::Code;
                }
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
enum CommentState {
    Code,
    Line,
    Block,
    DoubleQuote,
    SingleQuote,
    RawString,
}

fn should_compile_file(filename: &str, source: &str) -> bool {
    file_name_matches_target(filename) && build_constraint_matches(source)
}

fn file_name_matches_target(filename: &str) -> bool {
    let stem = filename.strip_suffix(".go").unwrap_or(filename);
    let parts = stem.split('_').collect::<Vec<_>>();
    let Some(last) = parts.last().copied() else {
        return true;
    };
    if goarch_names().contains(&last) {
        if last != "gors" {
            return false;
        }
        let os_part = parts.get(parts.len().saturating_sub(2)).copied();
        return os_part
            .is_none_or(|os_part| !goos_names().contains(&os_part) || os_part == host_goos());
    }
    !goos_names().contains(&last) || last == host_goos()
}

fn build_constraint_matches(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("//go:build ") {
            return eval_build_expr(expr);
        }
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        break;
    }
    true
}

fn eval_build_expr(expr: &str) -> bool {
    if expr.contains(" || ") {
        return expr.split(" || ").any(eval_build_expr);
    }
    if expr.contains(" && ") {
        return expr.split(" && ").all(eval_build_expr);
    }
    let expr = expr.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(inner) = expr.strip_prefix('!') {
        return !eval_build_expr(inner);
    }
    build_tag_matches(expr)
}

fn build_tag_matches(tag: &str) -> bool {
    tag == host_goos()
        || tag == "gors"
        || tag == "gc"
        || (tag == "unix" && is_unix_goos(host_goos()))
        || tag
            .strip_prefix("go1.")
            .and_then(|minor| minor.parse::<u32>().ok())
            .is_some_and(|minor| minor <= go_version_minor())
}

fn host_goos() -> &'static str {
    std::env::var("GOOS")
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| Box::leak(value.into_boxed_str()) as &'static str)
        .unwrap_or_else(|| match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        })
}

fn go_version_minor() -> u32 {
    gors::GO_VERSION
        .split('.')
        .nth(1)
        .and_then(|minor| minor.parse().ok())
        .unwrap_or(0)
}

fn is_unix_goos(goos: &str) -> bool {
    matches!(
        goos,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "solaris"
    )
}

fn goos_names() -> &'static [&'static str] {
    &[
        "aix",
        "android",
        "darwin",
        "dragonfly",
        "freebsd",
        "hurd",
        "illumos",
        "ios",
        "js",
        "linux",
        "netbsd",
        "openbsd",
        "plan9",
        "solaris",
        "wasip1",
        "windows",
    ]
}

fn goarch_names() -> &'static [&'static str] {
    &[
        "386", "amd64", "arm", "arm64", "loong64", "mips", "mips64", "mips64le", "mipsle", "ppc64",
        "ppc64le", "riscv64", "s390x", "wasm", "gors",
    ]
}

fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol_map(names: &[&str]) -> BTreeMap<String, BTreeMap<String, StdlibSymbol>> {
        let mut symbols = BTreeMap::new();
        for name in names {
            add_symbol(&mut symbols, "archive/tar", name, "method");
        }
        symbols
    }

    fn fixtures_for(
        symbols: &BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
        name: &str,
    ) -> BTreeSet<String> {
        symbols
            .get("archive/tar")
            .and_then(|package| package.get(name))
            .map(|symbol| symbol.fixtures.clone())
            .unwrap_or_default()
    }

    #[test]
    fn exported_symbol_parser_ignores_function_locals() {
        let source = r#"
package cipher

type Stream interface {
	XORKeyStream(dst, src []byte)
}

func NewCTR(block Block, iv []byte) Stream {
	var H, counter [16]byte
	_ = H
	_ = counter
	return nil
}
"#;

        let symbols = parse_exported_symbols(source);

        assert!(symbols.contains(&("Stream".to_string(), "type".to_string())));
        assert!(symbols.contains(&("NewCTR".to_string(), "func".to_string())));
        assert!(!symbols.iter().any(|(name, _)| name == "H"));
    }

    #[test]
    fn fixture_usage_marks_behavioral_coverage_markers() {
        let mut symbols = symbol_map(&["Header.FileInfo", "Reader.Next", "Writer.WriteHeader"]);
        let source = r#"
package main

import "archive/tar"

func main() {
	// gors:stdlib-cover archive/tar::Header.FileInfo archive/tar::Reader.Next
	// gors:stdlib-cover archive/tar::Writer.WriteHeader
}
"#;

        add_behavioral_fixture_usage(&mut symbols, source, "archive/tar").unwrap();

        assert_eq!(
            fixtures_for(&symbols, "Header.FileInfo"),
            BTreeSet::from(["archive/tar".to_string()])
        );
        assert_eq!(
            fixtures_for(&symbols, "Reader.Next"),
            BTreeSet::from(["archive/tar".to_string()])
        );
        assert_eq!(
            fixtures_for(&symbols, "Writer.WriteHeader"),
            BTreeSet::from(["archive/tar".to_string()])
        );
    }

    #[test]
    fn fixture_usage_ignores_selector_only_references() {
        let mut symbols = symbol_map(&["Format.String", "Writer.WriteHeader"]);
        let source = r#"
package main

import "archive/tar"

func coverArchiveTarAPI() {
	var _ = tar.Format.String
	var _ = (*tar.Writer).WriteHeader
}
"#;

        add_behavioral_fixture_usage(&mut symbols, source, "archive/tar").unwrap();

        assert!(fixtures_for(&symbols, "Format.String").is_empty());
        assert!(fixtures_for(&symbols, "Writer.WriteHeader").is_empty());
    }

    #[test]
    fn fixture_usage_reads_only_passed_fixtures() {
        let mut symbols = symbol_map(&["Header.FileInfo", "Writer.WriteHeader"]);
        let fixture_root = tempfile::tempdir().unwrap();
        let passed = fixture_root.path().join("archive/tar");
        let skipped = fixture_root.path().join("archive/zip");
        fs::create_dir_all(&passed).unwrap();
        fs::create_dir_all(&skipped).unwrap();
        fs::write(
            passed.join("main.go"),
            r#"
package main

func main() {
	// gors:stdlib-cover archive/tar::Header.FileInfo
}
"#,
        )
        .unwrap();
        fs::write(
            skipped.join("main.go"),
            r#"
package main

func main() {
	// gors:stdlib-cover archive/tar::Writer.WriteHeader
}
"#,
        )
        .unwrap();

        let fixtures = add_fixture_usage(
            fixture_root.path(),
            &["archive/tar".to_string()],
            &mut symbols,
        )
        .unwrap();

        assert_eq!(fixtures, vec!["archive/tar".to_string()]);
        assert_eq!(
            fixtures_for(&symbols, "Header.FileInfo"),
            BTreeSet::from(["archive/tar".to_string()])
        );
        assert!(fixtures_for(&symbols, "Writer.WriteHeader").is_empty());
    }

    #[test]
    fn fixture_usage_rejects_unknown_behavioral_coverage_markers() {
        let mut symbols = symbol_map(&["Format.String"]);
        let source = r#"
package main

func main() {
	// gors:stdlib-cover archive/tar::Missing
}
"#;

        let err = add_behavioral_fixture_usage(&mut symbols, source, "archive/tar").unwrap_err();
        assert_eq!(
            err,
            "archive/tar: behavioral coverage marker references unknown stdlib symbol archive/tar::Missing"
        );
    }
}
