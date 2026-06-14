#![cfg(feature = "test_integration_go_programs")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

#[test]
fn default_run_workers_oversubscribe_cpus() {
    assert_eq!(common::runner::default_run_workers_for_cpus(0), 2);
    assert_eq!(common::runner::default_run_workers_for_cpus(1), 2);
    assert_eq!(common::runner::default_run_workers_for_cpus(8), 16);
}

#[test]
fn run_go_programs_generated_rust() {
    common::runner::run_generated_program_fixture_set("go_programs");
}
