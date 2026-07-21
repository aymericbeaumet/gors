#![cfg(feature = "test_integration_go_stdlib")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn run_go_stdlib_generated_rust() {
    let fixture_run = common::runner::run_generated_program_fixture_set("go_stdlib");
    if common::reporter::canonical_report_requested() {
        assert!(
            fixture_run.complete,
            "refusing to regenerate the canonical Go stdlib report from a filtered, limited, diagnostic, or cancelled run"
        );
        common::reporter::write_go_stdlib_conformance(&fixture_run.passed_fixture_names)
            .expect("failed to write go-stdlib-conformance report");
    }
}
