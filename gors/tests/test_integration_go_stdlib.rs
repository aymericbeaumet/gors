#![cfg(feature = "test_integration_go_stdlib")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn run_go_stdlib_generated_rust() {
    let fixture_run = common::runner::run_generated_program_fixture_set("go_stdlib");
    common::reporter::write_go_stdlib_conformance(
        &fixture_run.passed_fixture_names,
        &fixture_run.attempted_fixture_names,
        fixture_run.retain_unattempted_fixture_names,
    )
    .expect("failed to write go-stdlib-conformance report");
}
