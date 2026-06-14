#![cfg(feature = "test_integration_go_repositories")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, collect_go_files, fixtures_dir, test_files_parallel};

fn repository_files() -> Vec<std::path::PathBuf> {
    let repos_dir = fixtures_dir().join("go_repositories");
    if !repos_dir.exists() {
        eprintln!("Skipping test_integration_go_repositories: no Go repository fixtures found");
        eprintln!("Run `git submodule update --init --recursive` to initialize test repositories");
        return Vec::new();
    }

    let mut files = collect_go_files(&repos_dir);
    files.sort();
    files
}

#[test]
fn test_integration_go_repositories_lexer() {
    let config = TestConfig::from_env();
    let files = repository_files();
    if files.is_empty() {
        eprintln!("Skipping test_integration_go_repositories_lexer: no .go files found");
        return;
    }

    eprintln!("Testing lexer on {} repository files...", files.len());
    let summary = test_files_parallel("tokens", &files, &config);
    summary.assert_all_passed();
}

#[test]
fn test_integration_go_repositories_parser() {
    let config = TestConfig::from_env();
    let files = repository_files();
    if files.is_empty() {
        eprintln!("Skipping test_integration_go_repositories_parser: no .go files found");
        return;
    }

    eprintln!("Testing parser on {} repository files...", files.len());
    let summary = test_files_parallel("ast", &files, &config);
    summary.assert_all_passed();
}
