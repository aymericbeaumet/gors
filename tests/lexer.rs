#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, collect_go_files, fixtures_dir, test_files_parallel};

#[test]
fn lexer_files() {
    let config = TestConfig::from_env();
    let files_dir = fixtures_dir().join("go_sources/files");
    let files = collect_go_files(&files_dir);
    assert!(
        !files.is_empty(),
        "No .go files found in fixtures/go_sources/files"
    );

    let summary = test_files_parallel("tokens", &files, &config);
    summary.assert_all_passed();
}

#[test]
fn lexer_programs() {
    let config = TestConfig::from_env();
    let programs_dir = fixtures_dir().join("go_programs");
    let files = collect_go_files(&programs_dir);
    assert!(
        !files.is_empty(),
        "No .go files found in fixtures/go_programs"
    );

    let summary = test_files_parallel("tokens", &files, &config);
    summary.assert_all_passed();
}

#[test]
#[cfg_attr(not(feature = "integration"), ignore)]
fn lexer_repositories() {
    let config = TestConfig::from_env();
    let repos_dir = fixtures_dir().join("go_sources/repositories");

    if !repos_dir.exists() {
        eprintln!("Skipping lexer_repositories: fixtures/go_sources/repositories not found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    let files = collect_go_files(&repos_dir);
    if files.is_empty() {
        eprintln!("Skipping lexer_repositories: no .go files found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    eprintln!("Testing lexer on {} repository files...", files.len());
    let summary = test_files_parallel("tokens", &files, &config);
    summary.assert_all_passed();
}
