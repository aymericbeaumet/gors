//! Parser and lexer tests.
//!
//! These tests compare gors parser/lexer output with the Go reference implementation.
//! Tests run in parallel for maximum performance.
//!
//! ## Environment Variables
//!
//! - `GORS_TEST_LIMIT`: Maximum number of files to test (default: unlimited)
//! - `GORS_TEST_FILTER`: Only test files matching this substring
//! - `GORS_TEST_VERBOSE`: Show progress during testing (set to "1" to enable)
//!
//! ## Examples
//!
//! ```bash
//! # Run all parser tests
//! cargo test --release parser
//!
//! # Run with a limit of 100 files
//! GORS_TEST_LIMIT=100 cargo test --release parser_repositories
//!
//! # Run only tests matching "kubernetes"
//! GORS_TEST_FILTER=kubernetes cargo test --release parser_repositories
//!
//! # Run with verbose progress output
//! GORS_TEST_VERBOSE=1 cargo test --release parser_repositories -- --nocapture
//! ```

mod common;

use common::{TestConfig, collect_go_files, fixtures_dir, test_files_parallel};

// =============================================================================
// Lexer Tests
// =============================================================================

/// Test lexer on files in fixtures/go_sources/files.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
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

/// Test lexer on programs in fixtures/go_programs.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
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

/// Test lexer on repositories.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
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

// =============================================================================
// Parser Tests
// =============================================================================

/// Test parser on files in fixtures/go_sources/files.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
fn parser_files() {
    let config = TestConfig::from_env();
    let files_dir = fixtures_dir().join("go_sources/files");
    let files = collect_go_files(&files_dir);
    assert!(
        !files.is_empty(),
        "No .go files found in fixtures/go_sources/files"
    );

    let summary = test_files_parallel("ast", &files, &config);
    summary.assert_all_passed();
}

/// Test parser on programs in fixtures/go_programs.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
fn parser_programs() {
    let config = TestConfig::from_env();
    let programs_dir = fixtures_dir().join("go_programs");
    let files = collect_go_files(&programs_dir);
    assert!(
        !files.is_empty(),
        "No .go files found in fixtures/go_programs"
    );

    let summary = test_files_parallel("ast", &files, &config);
    summary.assert_all_passed();
}

/// Test parser on repositories.
#[test]
#[ignore] // slow: run with `cargo test -- --ignored`
fn parser_repositories() {
    let config = TestConfig::from_env();
    let repos_dir = fixtures_dir().join("go_sources/repositories");

    if !repos_dir.exists() {
        eprintln!("Skipping parser_repositories: fixtures/go_sources/repositories not found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    let files = collect_go_files(&repos_dir);
    if files.is_empty() {
        eprintln!("Skipping parser_repositories: no .go files found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    eprintln!("Testing parser on {} repository files...", files.len());
    let summary = test_files_parallel("ast", &files, &config);
    summary.assert_all_passed();
}
