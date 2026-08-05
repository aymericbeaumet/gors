#![allow(clippy::unwrap_used, clippy::expect_used)]

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
fn spec_case_passes_only_with_fresh_fixture_evidence() {
    let make_case = || SpecCase {
        id: "maps-alias".to_string(),
        section: "Map types".to_string(),
        title: "Map assignment shares state".to_string(),
        fixtures: Some(vec!["types_map_alias".to_string()]),
        reason: None,
    };

    let without_evidence = spec_report_case(
        make_case(),
        &BTreeSet::new(),
        &BTreeSet::from(["types_map_alias".to_string()]),
    );
    let with_evidence = spec_report_case(
        make_case(),
        &BTreeSet::from(["types_map_alias".to_string()]),
        &BTreeSet::from(["types_map_alias".to_string()]),
    );

    assert_eq!(without_evidence.status, ReportStatus::Unsupported);
    assert!(!without_evidence.reason.is_empty());
    assert!(without_evidence.fixtures.is_empty());
    assert_eq!(with_evidence.status, ReportStatus::Passing);
    assert_eq!(with_evidence.fixtures, vec!["types_map_alias"]);
    assert!(with_evidence.reason.is_empty());
}

#[test]
fn report_case_deserializes_without_fixture_provenance() {
    let json = r#"
{
  "schemaVersion": 1,
  "kind": "go-stdlib",
  "title": "Go Standard Library Conformance",
  "source": {
"title": "The Go Standard Library",
"url": "",
"languageVersion": "go1.26.3",
"published": "",
"retrieved": ""
  },
  "summary": {
"groupCount": 1,
"passingGroupCount": 1,
"caseCount": 1,
"passingCaseCount": 1,
"unsupportedCaseCount": 0,
"fixtureCount": 1
  },
  "groups": [
{
  "id": "archive/tar",
  "title": "archive/tar",
  "subtitle": "",
  "fixtures": ["archive/tar"],
  "summary": {
    "groupCount": 0,
    "passingGroupCount": 0,
    "caseCount": 1,
    "passingCaseCount": 1,
    "unsupportedCaseCount": 0,
    "fixtureCount": 0
  },
  "cases": [
    {
      "id": "archive/tar::Header",
      "title": "Header",
      "subtitle": "type",
      "kind": "type",
      "status": "passing",
      "fixtures": ["archive/tar"],
      "reason": ""
    }
  ]
}
  ]
}
"#;

    let report: ConformanceReport = serde_json::from_str(json).unwrap();
    let group = report.groups.first().expect("report group");
    let case = group.cases.first().expect("report case");

    assert_eq!(case.fixtures, vec!["archive/tar"]);
    assert!(case.fresh_fixtures.is_empty());
    assert!(case.retained_fixtures.is_empty());
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

#[test]
fn unsupported_reasons_load_known_symbols() {
    let symbols = symbol_map(&["NewReader", "Reader.Next"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.json");
    fs::write(
        &path,
        r#"{
  "archive/tar::NewReader": "reader construction currently roots unsupported mutable io.Reader lowering",
  "archive/tar::Reader.Next": "reader iteration currently roots unsupported mutable []byte lvalue lowering"
}"#,
    )
    .unwrap();

    let reasons = load_stdlib_unsupported_reasons(&path, &symbols).unwrap();

    assert_eq!(
        reasons.get("archive/tar::NewReader").map(String::as_str),
        Some("reader construction currently roots unsupported mutable io.Reader lowering")
    );
    assert_eq!(
        reasons.get("archive/tar::Reader.Next").map(String::as_str),
        Some("reader iteration currently roots unsupported mutable []byte lvalue lowering")
    );
}

#[test]
fn unsupported_reasons_reject_unknown_symbols() {
    let symbols = symbol_map(&["NewReader"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.json");
    fs::write(&path, r#"{ "archive/tar::Missing": "not implemented" }"#).unwrap();

    let err = load_stdlib_unsupported_reasons(&path, &symbols).unwrap_err();

    assert!(
        err.contains("unsupported reason references unknown stdlib symbol archive/tar::Missing")
    );
}

#[test]
fn unsupported_reasons_reject_empty_reasons() {
    let symbols = symbol_map(&["NewReader"]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.json");
    fs::write(&path, r#"{ "archive/tar::NewReader": " " }"#).unwrap();

    let err = load_stdlib_unsupported_reasons(&path, &symbols).unwrap_err();

    assert!(err.contains("unsupported reason for archive/tar::NewReader must not be empty"));
}

#[test]
fn unsupported_reasons_reject_stale_passing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.json");
    let reasons = BTreeMap::from([(
        "archive/tar::NewReader".to_string(),
        "reader path is not covered".to_string(),
    )]);

    let err = reject_stale_unsupported_reasons(&path, &reasons).unwrap_err();

    assert!(err.contains("unsupported reason for passing stdlib symbol archive/tar::NewReader"));
}

#[test]
fn unsupported_reason_defaults_for_go_internal_packages() {
    let mut reasons = BTreeMap::new();

    assert_eq!(
        stdlib_unsupported_reason(
            "crypto/internal/fips140/aes",
            "crypto/internal/fips140/aes::New",
            &mut reasons
        ),
        GO_INTERNAL_UNSUPPORTED_REASON
    );
    assert_eq!(
        stdlib_unsupported_reason(
            "internal/testenv",
            "internal/testenv::Builder",
            &mut reasons
        ),
        GO_INTERNAL_UNSUPPORTED_REASON
    );
}

#[test]
fn unsupported_reason_defaults_for_go_asm_generator_packages() {
    let mut reasons = BTreeMap::new();

    assert_eq!(
        stdlib_unsupported_reason("crypto/md5/_asm", "crypto/md5/_asm::ROUND1", &mut reasons),
        GO_ASM_GENERATOR_UNSUPPORTED_REASON
    );
    assert_eq!(
        stdlib_unsupported_reason(
            "crypto/internal/fips140/sha256/_asm",
            "crypto/internal/fips140/sha256/_asm::Round",
            &mut reasons
        ),
        GO_ASM_GENERATOR_UNSUPPORTED_REASON
    );
}

#[test]
fn unsupported_reason_never_defaults_to_empty() {
    let mut reasons = BTreeMap::new();

    assert_eq!(
        stdlib_unsupported_reason("archive/tar", "archive/tar::NewReader", &mut reasons),
        NO_FRESH_STDLIB_EVIDENCE_REASON
    );
}

#[test]
fn unsupported_reason_keeps_explicit_internal_package_reason() {
    let mut reasons = BTreeMap::from([(
        "crypto/internal/fips140/aes::New".to_string(),
        "AES lowering still needs generic array-addressability support".to_string(),
    )]);

    assert_eq!(
        stdlib_unsupported_reason(
            "crypto/internal/fips140/aes",
            "crypto/internal/fips140/aes::New",
            &mut reasons
        ),
        "AES lowering still needs generic array-addressability support"
    );
    assert!(reasons.is_empty());
}
