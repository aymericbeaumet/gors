#![cfg(feature = "test_integration_lexer")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gors::test_support::{TestConfig, collect_go_files, fixtures_dir, test_files_parallel};

#[test]
fn test_integration_lexer() {
    let config = TestConfig::from_env();
    let repos_dir = fixtures_dir().join("go_sources/repositories");

    if !repos_dir.exists() {
        eprintln!("Skipping test_integration_lexer: fixtures/go_sources/repositories not found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    let files = collect_go_files(&repos_dir);
    if files.is_empty() {
        eprintln!("Skipping test_integration_lexer: no .go files found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    eprintln!("Testing lexer on {} repository files...", files.len());
    let summary = test_files_parallel("tokens", &files, &config);
    summary.assert_all_passed();
}
