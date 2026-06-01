#![cfg(feature = "test_integration_go_stdlib")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn run_go_stdlib_generated_rust() {
    common::runner::run_generated_program_fixture_set("go_stdlib");
    common::reporter::write_go_stdlib_conformance()
        .expect("failed to write go-stdlib-conformance report");
}
