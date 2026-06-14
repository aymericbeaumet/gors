#![cfg(feature = "test_integration_go_spec")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::fixtures_dir;
use std::collections::HashSet;
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
    reason: Option<String>,
    expect: Option<String>,
}

#[derive(serde::Deserialize)]
struct SourceBytesFixture {
    filename: Option<String>,
    source: String,
}

const GO_1_26_SPEC_SECTIONS: &[&str] = &[
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
    let mut ids = HashSet::new();
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
            let fixtures = case.fixtures.unwrap_or_default();
            match case.status.as_str() {
                "passing" => {
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
                "unsupported" => {
                    assert!(
                        case.reason.is_some_and(|reason| !reason.trim().is_empty()),
                        "unsupported spec test {} has no reason",
                        case.id
                    );
                    for fixture in fixtures {
                        assert!(
                            go_spec.join(&fixture).join("main.go").exists(),
                            "unsupported spec test {} references missing fixture {}",
                            case.id,
                            fixture
                        );
                    }
                }
                other => panic!("spec test {} has invalid status {}", case.id, other),
            }
        }
    }

    assert!(total > 0, "spec manifest is empty");
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

#[test]
fn go_spec_compile_error_fixtures_reject_like_go() {
    let manifest = read_spec_manifest();
    let go_spec = fixtures_dir().join("go_spec");
    let mut checked = 0usize;

    for category in manifest.categories {
        for case in category.tests {
            if case.status != "passing" || case.expect.as_deref() != Some("compile_error") {
                continue;
            }
            for fixture in case.fixtures.unwrap_or_default() {
                checked += 1;
                assert_compile_error_fixture(&go_spec.join(&fixture), &case.id);
            }
        }
    }

    assert!(checked > 0, "no compile-error spec fixtures found");
}

#[test]
fn go_spec_source_byte_fixtures_match_go() {
    let manifest = read_spec_manifest();
    let go_spec = fixtures_dir().join("go_spec");
    let mut checked = 0usize;

    for category in manifest.categories {
        for case in category.tests {
            if case.status != "passing" || case.expect.as_deref() != Some("source_bytes") {
                continue;
            }
            for fixture in case.fixtures.unwrap_or_default() {
                checked += 1;
                assert_source_bytes_fixture_matches_go(
                    &go_spec.join(&fixture).join("source.json"),
                    &case.id,
                );
            }
        }
    }

    assert!(checked > 0, "no source-bytes spec fixtures found");
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
    let go_output = common::go_command()
        .args(["run", filename])
        .current_dir(tempdir.path())
        .output()
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
    let go_output = common::go_command()
        .args(["run", "."])
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("{case_id}: failed to run Go oracle: {e}"));
    assert!(
        !go_output.status.success(),
        "{case_id}: Go accepted negative fixture {}",
        dir.display()
    );

    let source_path = dir.to_string_lossy().into_owned();
    let rejected = match gors::parser::parse_program_files(&[source_path]) {
        Ok(program) => gors::compiler::compile_program_multi(program).is_err(),
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
    common::runner::run_generated_program_fixture_set("go_spec");
    common::reporter::write_go_spec_conformance()
        .expect("failed to write go-spec-conformance report");
}
