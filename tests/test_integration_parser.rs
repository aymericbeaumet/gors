#![cfg(feature = "test_integration_parser")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{TestConfig, collect_go_files, fixtures_dir, test_files_parallel};

#[test]
fn test_integration_parser() {
    let config = TestConfig::from_env();
    let files_dir = fixtures_dir().join("go_files");
    let repos_dir = fixtures_dir().join("go_repositories");

    if !files_dir.exists() && !repos_dir.exists() {
        eprintln!("Skipping test_integration_parser: no Go file fixtures found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    let mut files = collect_go_files(&files_dir);
    files.extend(collect_go_files(&repos_dir));
    files.sort();
    if files.is_empty() {
        eprintln!("Skipping test_integration_parser: no .go files found");
        eprintln!("Run `make setup` to initialize test repositories");
        return;
    }

    eprintln!("Testing parser on {} repository files...", files.len());
    let summary = test_files_parallel("ast", &files, &config);
    summary.assert_all_passed();
}
