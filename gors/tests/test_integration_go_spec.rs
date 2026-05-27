#![cfg(feature = "test_integration_go_spec")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
#[path = "support/generated_programs.rs"]
mod generated_programs;

use common::fixtures_dir;
use std::collections::HashSet;
use std::fs;

#[derive(serde::Deserialize)]
struct SpecManifest {
    categories: Vec<SpecCategory>,
}

#[derive(serde::Deserialize)]
struct SpecCategory {
    name: String,
    tests: Vec<SpecCase>,
}

#[derive(serde::Deserialize)]
struct SpecCase {
    id: String,
    status: String,
    fixtures: Option<Vec<String>>,
    reason: Option<String>,
}

#[test]
fn go_spec_manifest_has_valid_statuses_and_fixtures() {
    let manifest_path = fixtures_dir().join("go_spec/spec.json");
    let manifest: SpecManifest = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {}", manifest_path.display(), e)),
    )
    .unwrap_or_else(|e| panic!("cannot parse {}: {}", manifest_path.display(), e));
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
fn run_go_spec_generated_rust() {
    generated_programs::run_generated_program_fixture_set("go_spec");
}
