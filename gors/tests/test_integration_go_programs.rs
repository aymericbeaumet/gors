#![cfg(feature = "test_integration_go_programs")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn default_run_workers_respect_cpu_budget() {
    assert_eq!(common::runner::default_run_workers_for_cpus(0), 1);
    assert_eq!(common::runner::default_run_workers_for_cpus(1), 1);
    assert_eq!(common::runner::default_run_workers_for_cpus(8), 8);
}

#[test]
fn run_go_programs_generated_rust() {
    common::runner::run_generated_program_fixture_set("go_programs");
}
