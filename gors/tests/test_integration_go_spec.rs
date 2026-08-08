#![cfg(feature = "test_integration_go_spec")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fixtures_dir;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(serde::Deserialize)]
struct SpecManifest {
    source: SpecSource,
    categories: Vec<SpecCategory>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpecSource {
    title: String,
    url: String,
    language_version: String,
    published: String,
    retrieved: String,
}

#[derive(serde::Deserialize)]
struct SpecCategory {
    name: String,
    tests: Vec<SpecCase>,
}

#[derive(serde::Deserialize)]
struct SpecCase {
    id: String,
    section: String,
    title: String,
    status: String,
    fixtures: Option<Vec<String>>,
    expect: Option<String>,
}

#[derive(serde::Deserialize)]
struct SourceBytesFixture {
    filename: Option<String>,
    source: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureExecutionManifest {
    fixtures: HashMap<String, FixtureExecutionDirective>,
}

#[derive(serde::Deserialize)]
struct FixtureExecutionDirective {
    status: String,
}

const EXPECTED_SPEC_CASE_COUNT: usize = 348;

const GO_1_26_SPEC_SECTIONS: &[&str] = &[
    "Language versions",
    "Source code representation",
    "Characters",
    "Letters and digits",
    "Lexical elements",
    "Comments",
    "Tokens",
    "Semicolons",
    "Identifiers",
    "Keywords",
    "Operators and punctuation",
    "Integer literals",
    "Floating-point literals",
    "Imaginary literals",
    "Rune literals",
    "String literals",
    "Constants",
    "Variables",
    "Types",
    "Boolean types",
    "Numeric types",
    "String types",
    "Array types",
    "Slice types",
    "Struct types",
    "Pointer types",
    "Function types",
    "Interface types",
    "Map types",
    "Channel types",
    "Properties of types and values",
    "Representation of values",
    "Underlying types",
    "Type identity",
    "Assignability",
    "Representability",
    "Method sets",
    "Blocks",
    "Declarations and scope",
    "Label scopes",
    "Blank identifier",
    "Predeclared identifiers",
    "Exported identifiers",
    "Uniqueness of identifiers",
    "Constant declarations",
    "Iota",
    "Type declarations",
    "Type parameter declarations",
    "Variable declarations",
    "Short variable declarations",
    "Function declarations",
    "Method declarations",
    "Expressions",
    "Operands",
    "Qualified identifiers",
    "Composite literals",
    "Function literals",
    "Primary expressions",
    "Selectors",
    "Method expressions",
    "Method values",
    "Index expressions",
    "Slice expressions",
    "Type assertions",
    "Calls",
    "Passing arguments to ... parameters",
    "Instantiations",
    "Type inference",
    "Operators",
    "Arithmetic operators",
    "Comparison operators",
    "Logical operators",
    "Address operators",
    "Receive operator",
    "Conversions",
    "Constant expressions",
    "Order of evaluation",
    "Statements",
    "Terminating statements",
    "Empty statements",
    "Labeled statements",
    "Expression statements",
    "Send statements",
    "IncDec statements",
    "Assignment statements",
    "If statements",
    "Switch statements",
    "For statements",
    "Go statements",
    "Select statements",
    "Return statements",
    "Break statements",
    "Continue statements",
    "Goto statements",
    "Fallthrough statements",
    "Defer statements",
    "Built-in functions",
    "Appending to and copying slices",
    "Clear",
    "Close",
    "Manipulating complex numbers",
    "Deletion of map elements",
    "Length and capacity",
    "Making slices, maps and channels",
    "Min and max",
    "Allocation",
    "Handling panics",
    "Bootstrapping",
    "Packages",
    "Source file organization",
    "Package clause",
    "Import declarations",
    "An example package",
    "Program initialization and execution",
    "The zero value",
    "Package initialization",
    "Program initialization",
    "Program execution",
    "Errors",
    "Run-time panics",
    "System considerations",
    "Package unsafe",
    "Size and alignment guarantees",
    "Type unification rules",
];

fn read_spec_manifest() -> SpecManifest {
    let manifest_path = fixtures_dir().join("go_spec/spec.json");
    serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", manifest_path.display(), e)),
    )
    .unwrap_or_else(|e| panic!("cannot parse {}: {}", manifest_path.display(), e))
}

#[test]
fn go_spec_manifest_has_valid_statuses_and_fixtures() {
    let manifest = read_spec_manifest();
    let go_spec = fixtures_dir().join("go_spec");
    let execution_manifest_path = go_spec.join("fixtures.json");
    let execution_manifest: FixtureExecutionManifest = serde_json::from_str(
        &fs::read_to_string(&execution_manifest_path).unwrap_or_else(|error| {
            panic!("cannot read {}: {error}", execution_manifest_path.display())
        }),
    )
    .unwrap_or_else(|error| {
        panic!(
            "cannot parse {}: {error}",
            execution_manifest_path.display()
        )
    });
    let mut ids = HashSet::new();
    let mut referenced_fixtures = HashSet::new();
    let mut total = 0usize;

    for category in manifest.categories {
        assert!(!category.name.trim().is_empty(), "empty spec category");
        assert!(
            !category.tests.is_empty(),
            "spec category {} has no tests",
            category.name
        );
        for case in category.tests {
            total += 1;
            assert!(
                !case.section.trim().is_empty(),
                "spec test {} has an empty section",
                case.id
            );
            assert!(
                !case.title.trim().is_empty(),
                "spec test {} has an empty title",
                case.id
            );
            assert!(
                ids.insert(case.id.clone()),
                "duplicate spec test id {}",
                case.id
            );
            assert_eq!(
                case.status, "passing",
                "spec test {} must be supported; unsupported cases are not permitted",
                case.id
            );
            let fixtures = case.fixtures.unwrap_or_default();
            referenced_fixtures.extend(fixtures.iter().cloned());
            for fixture in &fixtures {
                let execution_status = execution_manifest
                    .fixtures
                    .get(fixture)
                    .map(|directive| directive.status.as_str());
                match case.expect.as_deref() {
                    Some("compile_error") => assert_eq!(
                        execution_status,
                        Some("compile_error"),
                        "compile-error spec test {} fixture {} must have an explicit compile_error execution status",
                        case.id,
                        fixture
                    ),
                    _ => assert!(
                        !matches!(execution_status, Some("unsupported" | "compile_error")),
                        "passing spec test {} fixture {} has non-running execution status {:?}",
                        case.id,
                        fixture,
                        execution_status
                    ),
                }
            }
            assert!(
                !fixtures.is_empty(),
                "passing spec test {} has no fixtures",
                case.id
            );
            for fixture in fixtures {
                if case.expect.as_deref() == Some("source_bytes") {
                    assert!(
                        go_spec.join(&fixture).join("source.json").exists(),
                        "passing source-bytes spec test {} references missing fixture {}",
                        case.id,
                        fixture
                    );
                    continue;
                }
                assert!(
                    go_spec.join(&fixture).join("main.go").exists(),
                    "passing spec test {} references missing fixture {}",
                    case.id,
                    fixture
                );
            }
        }
    }

    assert_eq!(
        total, EXPECTED_SPEC_CASE_COUNT,
        "Go spec manifest must contain exactly {EXPECTED_SPEC_CASE_COUNT} supported cases"
    );
    for (fixture, directive) in &execution_manifest.fixtures {
        assert_ne!(
            directive.status, "unsupported",
            "Go spec fixture {} must not have an unsupported execution status",
            fixture
        );
    }
    let mut executable_fixtures = Vec::new();
    collect_fixture_directories_with_file(&go_spec, &go_spec, "main.go", &mut executable_fixtures);
    collect_fixture_directories_with_file(
        &go_spec,
        &go_spec,
        "source.json",
        &mut executable_fixtures,
    );
    let unreferenced = executable_fixtures
        .into_iter()
        .filter(|fixture| !referenced_fixtures.contains(fixture))
        .collect::<Vec<_>>();
    assert!(
        unreferenced.is_empty(),
        "Go spec fixtures without manifest evidence cases: {}",
        unreferenced.join(", ")
    );
}

fn collect_fixture_directories_with_file(
    root: &Path,
    dir: &Path,
    filename: &str,
    fixtures: &mut Vec<String>,
) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot enumerate {}: {error}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|error| panic!("cannot enumerate {}: {error}", dir.display()));
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join(filename).exists() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
                .to_string_lossy()
                .replace('\\', "/");
            fixtures.push(relative);
        }
        collect_fixture_directories_with_file(root, &path, filename, fixtures);
    }
}

#[test]
fn go_spec_manifest_covers_go_1_26_language_sections() {
    let manifest = read_spec_manifest();
    assert_eq!(
        manifest.source.title,
        "The Go Programming Language Specification"
    );
    assert_eq!(manifest.source.url, "https://go.dev/ref/spec");
    assert_eq!(manifest.source.language_version, "go1.26");
    assert_eq!(manifest.source.published, "2026-01-12");
    assert!(
        !manifest.source.retrieved.trim().is_empty(),
        "spec manifest source retrieval date is empty"
    );

    let mut covered = HashSet::new();
    for category in &manifest.categories {
        covered.insert(category.name.as_str());
        for case in &category.tests {
            covered.insert(case.section.as_str());
        }
    }
    let missing = GO_1_26_SPEC_SECTIONS
        .iter()
        .copied()
        .filter(|section| !covered.contains(section))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "spec manifest is missing Go 1.26 sections: {}",
        missing.join(", ")
    );
}

fn go_spec_compile_error_fixtures_reject_like_go(filter: Option<&str>) -> Vec<String> {
    let manifest = read_spec_manifest();
    let go_spec = fixtures_dir().join("go_spec");
    let mut passed_fixtures = Vec::new();

    for category in manifest.categories {
        for case in category.tests {
            if case.status != "passing" || case.expect.as_deref() != Some("compile_error") {
                continue;
            }
            let fixtures = case.fixtures.unwrap_or_default();
            if filter.is_some_and(|filter| {
                !case.id.contains(filter)
                    && !fixtures.iter().any(|fixture| fixture.contains(filter))
            }) {
                continue;
            }
            for fixture in fixtures {
                assert_compile_error_fixture(&go_spec.join(&fixture), &case.id);
                passed_fixtures.push(fixture);
            }
        }
    }

    if filter.is_none() {
        assert!(
            !passed_fixtures.is_empty(),
            "no compile-error spec fixtures found"
        );
    }
    passed_fixtures
}

fn go_spec_source_byte_fixtures_match_go(filter: Option<&str>) -> Vec<String> {
    let manifest = read_spec_manifest();
    let go_spec = fixtures_dir().join("go_spec");
    let mut passed_fixtures = Vec::new();

    for category in manifest.categories {
        for case in category.tests {
            if case.status != "passing" || case.expect.as_deref() != Some("source_bytes") {
                continue;
            }
            let fixtures = case.fixtures.unwrap_or_default();
            if filter.is_some_and(|filter| {
                !case.id.contains(filter)
                    && !fixtures.iter().any(|fixture| fixture.contains(filter))
            }) {
                continue;
            }
            for fixture in fixtures {
                assert_source_bytes_fixture_matches_go(
                    &go_spec.join(&fixture).join("source.json"),
                    &case.id,
                );
                passed_fixtures.push(fixture);
            }
        }
    }

    if filter.is_none() {
        assert!(
            !passed_fixtures.is_empty(),
            "no source-bytes spec fixtures found"
        );
    }
    passed_fixtures
}

fn assert_source_bytes_fixture_matches_go(path: &Path, case_id: &str) {
    let fixture: SourceBytesFixture = serde_json::from_str(
        &fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));
    let filename = fixture.filename.as_deref().unwrap_or("main.go");
    let tempdir =
        tempfile::tempdir().unwrap_or_else(|e| panic!("{case_id}: failed to create tempdir: {e}"));
    fs::write(tempdir.path().join(filename), fixture.source.as_bytes())
        .unwrap_or_else(|e| panic!("{case_id}: failed to write source bytes: {e}"));
    let mut go_command = common::go_command();
    go_command
        .args(["run", filename])
        .current_dir(tempdir.path())
        .stdin(std::process::Stdio::null());
    let go_output = common::runner::command_output_with_timeout(
        go_command,
        common::runner::configured_go_run_timeout(),
    )
    .unwrap_or_else(|e| panic!("{case_id}: failed to run Go oracle: {e}"));
    let go_accepts = go_output.status.success();
    let gors_accepts = gors::parser::parse_file(filename, &fixture.source).is_ok();

    assert_eq!(
        gors_accepts,
        go_accepts,
        "{case_id}: gors source-byte acceptance differed from Go for {}",
        path.display()
    );
}

fn assert_compile_error_fixture(dir: &Path, case_id: &str) {
    let mut go_command = common::go_command();
    go_command
        .args(["run", "."])
        .current_dir(dir)
        .stdin(std::process::Stdio::null());
    let go_output = common::runner::command_output_with_timeout(
        go_command,
        common::runner::configured_go_run_timeout(),
    )
    .unwrap_or_else(|e| panic!("{case_id}: failed to run Go oracle: {e}"));
    assert!(
        !go_output.status.success(),
        "{case_id}: Go accepted negative fixture {}",
        dir.display()
    );

    let workspace = gors::compiler::input::WorkspaceKey::ad_hoc("gors-go-spec-negative-fixtures")
        .expect("negative fixture workspace identity is valid");
    let rejected = match gors::workspace::load_program_files_auto(workspace, &[dir]) {
        Ok(program) => gors::compiler::compile_program(program.into_input()).is_err(),
        Err(_) => true,
    };
    assert!(
        rejected,
        "{case_id}: gors accepted negative fixture {}",
        dir.display()
    );
}

#[test]
fn run_go_spec_generated_rust() {
    // Keep report generation self-contained: a parallel manifest-validation test
    // must not be the only thing preventing missing evidence from reaching disk.
    go_spec_manifest_has_valid_statuses_and_fixtures();
    let filter = std::env::var("GORS_TEST_FILTER").ok();
    let mut fixture_run = common::runner::run_generated_program_fixture_set_allow_empty("go_spec");
    fixture_run.add_passing_evidence(go_spec_compile_error_fixtures_reject_like_go(
        filter.as_deref(),
    ));
    fixture_run.add_passing_evidence(go_spec_source_byte_fixtures_match_go(filter.as_deref()));
    assert!(
        !fixture_run.attempted_fixture_names.is_empty(),
        "no Go spec fixtures matched filter {:?}",
        filter
    );
    if common::reporter::canonical_report_requested() {
        assert!(
            fixture_run.complete,
            "refusing to regenerate the canonical Go spec report from a filtered, limited, diagnostic, or cancelled run"
        );
        common::reporter::write_go_spec_conformance(
            &fixture_run.passed_fixture_names,
            &fixture_run.attempted_fixture_names,
        )
        .expect("failed to write go-spec-conformance report");
    }
}
