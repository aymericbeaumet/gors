#![cfg(feature = "test_integration_go_stdlib")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
#[path = "support/generated_programs.rs"]
mod generated_programs;

#[test]
fn run_go_stdlib_generated_rust() {
    generated_programs::run_generated_program_fixture_set("go_stdlib");
}
