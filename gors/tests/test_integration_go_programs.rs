#![cfg(feature = "test_integration_go_programs")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
#[path = "support/generated_programs.rs"]
mod generated_programs;

#[test]
fn default_run_workers_oversubscribe_cpus() {
    assert_eq!(generated_programs::default_run_workers_for_cpus(0), 2);
    assert_eq!(generated_programs::default_run_workers_for_cpus(1), 2);
    assert_eq!(generated_programs::default_run_workers_for_cpus(8), 16);
}

#[test]
fn run_go_programs_generated_rust() {
    generated_programs::run_generated_program_fixture_set("go_programs");
}
