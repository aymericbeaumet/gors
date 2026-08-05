use crate::common::{fixtures_dir, workspace_root};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

mod source_analysis;

use source_analysis::{parse_exported_symbols, should_compile_file};

const REPORT_SCHEMA_VERSION: u32 = 1;
const GO_INTERNAL_UNSUPPORTED_REASON: &str = "Go internal package visibility: generated-program fixtures outside the parent tree cannot import this package directly; cover exported behavior through an importing parent package or add package-local harness support.";
const GO_ASM_GENERATOR_UNSUPPORTED_REASON: &str = "Go assembly generator package: this package is go:generate support code for assembly output, so generated-program coverage should exercise the compiled parent package behavior instead of the generator program.";
const NO_FRESH_STDLIB_EVIDENCE_REASON: &str =
    "No fresh passing behavioral generated-program evidence was recorded for this exported symbol.";

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fresh_fixtures: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retained_fixtures: Vec<String>,
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
    fixtures: Option<Vec<String>>,
    reason: Option<String>,
}

#[derive(Debug)]
struct StdlibSymbol {
    name: String,
    kind: String,
    fixtures: BTreeSet<String>,
}

pub fn canonical_report_requested() -> bool {
    std::env::var("GORS_UPDATE_CONFORMANCE_REPORTS")
        .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

pub fn write_go_spec_conformance(
    passed_fixture_names: &[String],
    attempted_fixture_names: &[String],
) -> Result<(), String> {
    let manifest_path = fixtures_dir().join("go_spec/spec.json");
    let manifest: SpecManifest = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?,
    )
    .map_err(|e| format!("cannot parse {}: {e}", manifest_path.display()))?;

    let passed = passed_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let attempted = attempted_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let groups = manifest
        .categories
        .into_iter()
        .map(|category| {
            let cases = category
                .tests
                .into_iter()
                .map(|case| spec_report_case(case, &passed, &attempted))
                .collect::<Vec<_>>();
            let summary = summarize_cases(&cases, collect_case_fixtures(&cases).len());
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
        summary: {
            let mut summary = summarize_groups(&groups);
            summary.fixture_count = passed.len();
            summary
        },
        groups,
    };
    write_report("go-spec-conformance.json", &report)
}

fn spec_report_case(
    case: SpecCase,
    passed: &BTreeSet<String>,
    attempted: &BTreeSet<String>,
) -> ReportCase {
    let declared_fixtures = case.fixtures.unwrap_or_default();
    let fresh_fixtures = declared_fixtures
        .iter()
        .filter(|fixture| passed.contains(*fixture))
        .cloned()
        .collect::<Vec<_>>();
    let missing_fixtures = declared_fixtures
        .iter()
        .filter(|fixture| !passed.contains(*fixture))
        .cloned()
        .collect::<Vec<_>>();
    let status = if !declared_fixtures.is_empty() && missing_fixtures.is_empty() {
        ReportStatus::Passing
    } else {
        ReportStatus::Unsupported
    };
    let reason = if status == ReportStatus::Passing {
        String::new()
    } else if let Some(reason) = case.reason.filter(|reason| !reason.trim().is_empty()) {
        reason
    } else if declared_fixtures.is_empty() {
        "No executable fixture is registered for this specification case.".to_string()
    } else {
        let attempted_but_not_passing = missing_fixtures
            .iter()
            .filter(|fixture| attempted.contains(*fixture))
            .cloned()
            .collect::<Vec<_>>();
        if attempted_but_not_passing.is_empty() {
            format!(
                "No fresh execution evidence was recorded for fixtures: {}.",
                missing_fixtures.join(", ")
            )
        } else {
            format!(
                "Fresh fixture execution did not pass for: {}.",
                attempted_but_not_passing.join(", ")
            )
        }
    };
    ReportCase {
        id: case.id,
        title: case.title,
        subtitle: case.section,
        kind: "spec-test".to_string(),
        status,
        fixtures: fresh_fixtures.clone(),
        fresh_fixtures,
        retained_fixtures: Vec::new(),
        reason,
    }
}

pub fn write_go_stdlib_conformance(passed_fixture_names: &[String]) -> Result<(), String> {
    let fixture_root = fixtures_dir().join("go_stdlib");
    let mut symbols_by_package = collect_stdlib_symbols()?;
    let mut unsupported_reasons = load_stdlib_unsupported_reasons(
        &fixture_root.join("unsupported.json"),
        &symbols_by_package,
    )?;
    let fresh_passed_fixture_names = passed_fixture_names
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let passed_fixture_names = fresh_passed_fixture_names
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let _fixture_names = add_fixture_usage(
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
                    let id = format!("{package_path}::{}", symbol.name);
                    let reason = if status == ReportStatus::Unsupported {
                        stdlib_unsupported_reason(&package_path, &id, &mut unsupported_reasons)
                    } else {
                        String::new()
                    };
                    ReportCase {
                        id,
                        title: symbol.name,
                        subtitle: symbol.kind.clone(),
                        kind: symbol.kind,
                        status,
                        fresh_fixtures: fixtures.clone(),
                        fixtures,
                        retained_fixtures: Vec::new(),
                        reason,
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
    reject_stale_unsupported_reasons(&fixture_root.join("unsupported.json"), &unsupported_reasons)?;

    let mut summary = summarize_groups(&groups);
    summary.fixture_count = fresh_passed_fixture_names.len();
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
    let mut temporary =
        tempfile::NamedTempFile::new_in(report_dir).map_err(|error| error.to_string())?;
    temporary
        .write_all(format!("{json}\n").as_bytes())
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(&path)
        .map_err(|error| error.error.to_string())?;
    eprintln!("Wrote {}", path.display());
    Ok(())
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

fn load_stdlib_unsupported_reasons(
    path: &Path,
    symbols_by_package: &BTreeMap<String, BTreeMap<String, StdlibSymbol>>,
) -> Result<BTreeMap<String, String>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let reasons: BTreeMap<String, String> = serde_json::from_str(
        &fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    for (id, reason) in &reasons {
        if reason.trim().is_empty() {
            return Err(format!(
                "{}: unsupported reason for {id} must not be empty",
                path.display()
            ));
        }
        let Some((package_path, symbol_name)) = id.split_once("::") else {
            return Err(format!(
                "{}: malformed unsupported reason id {id}",
                path.display()
            ));
        };
        if !symbols_by_package
            .get(package_path)
            .is_some_and(|symbols| symbols.contains_key(symbol_name))
        {
            return Err(format!(
                "{}: unsupported reason references unknown stdlib symbol {id}",
                path.display()
            ));
        }
    }
    Ok(reasons)
}

fn reject_stale_unsupported_reasons(
    path: &Path,
    reasons: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(id) = reasons.keys().next() {
        return Err(format!(
            "{}: unsupported reason for passing stdlib symbol {id}",
            path.display()
        ));
    }
    Ok(())
}

fn stdlib_unsupported_reason(
    package_path: &str,
    id: &str,
    unsupported_reasons: &mut BTreeMap<String, String>,
) -> String {
    unsupported_reasons
        .remove(id)
        .or_else(|| default_stdlib_unsupported_reason(package_path))
        .unwrap_or_else(|| NO_FRESH_STDLIB_EVIDENCE_REASON.to_string())
}

fn default_stdlib_unsupported_reason(package_path: &str) -> Option<String> {
    if is_go_asm_generator_package(package_path) {
        Some(GO_ASM_GENERATOR_UNSUPPORTED_REASON.to_string())
    } else if is_go_internal_package(package_path) {
        Some(GO_INTERNAL_UNSUPPORTED_REASON.to_string())
    } else {
        None
    }
}

fn is_go_internal_package(package_path: &str) -> bool {
    package_path.split('/').any(|part| part == "internal")
}

fn is_go_asm_generator_package(package_path: &str) -> bool {
    package_path.split('/').any(|part| part == "_asm")
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
mod tests;
