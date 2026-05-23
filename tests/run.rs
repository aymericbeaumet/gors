#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{discover_program_dirs, fixtures_dir, go_runner_bin, gors_bin};
use std::process::Command;

#[test]
fn run_single_file() {
    let gors = gors_bin();
    let main_go = fixtures_dir()
        .join("go_programs/hello_world/main.go")
        .to_string_lossy()
        .to_string();

    let gors_out = Command::new(gors.as_path())
        .args(["run", &main_go])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run main.go failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(fixtures_dir().join("go_programs/hello_world"))
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

#[test]
fn run_multiple_files() {
    let gors = gors_bin();
    let dir = fixtures_dir().join("go_programs/multi_file_same_package");
    let main_go = dir.join("main.go").to_string_lossy().to_string();
    let helpers_go = dir.join("helpers.go").to_string_lossy().to_string();

    let gors_out = Command::new(gors.as_path())
        .args(["run", &main_go, &helpers_go])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run main.go helpers.go failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

#[test]
fn run_current_directory() {
    let gors = gors_bin();
    let dir = fixtures_dir().join("go_programs/multi_file_same_package");

    let gors_out = Command::new(gors.as_path())
        .args(["run", dir.to_str().unwrap()])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run <dir> failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "."])
        .current_dir(&dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

#[test]
fn run_specific_package() {
    let gors = gors_bin();
    let pkg_dir = fixtures_dir().join("go_programs/cmd_layout/cmd/myapp");

    let gors_out = Command::new(gors.as_path())
        .args(["run", pkg_dir.to_str().unwrap()])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors: {e}"));

    assert!(
        gors_out.status.success(),
        "gors run ./cmd/myapp failed:\n{}",
        String::from_utf8_lossy(&gors_out.stderr),
    );

    let go_out = Command::new("go")
        .args(["run", "./cmd/myapp"])
        .current_dir(fixtures_dir().join("go_programs/cmd_layout"))
        .output()
        .unwrap_or_else(|e| panic!("failed to run go: {e}"));

    assert_eq!(
        String::from_utf8_lossy(&gors_out.stdout),
        String::from_utf8_lossy(&go_out.stdout),
    );
}

#[test]
fn build_removes_manifest_stale_output_files() {
    let gors = gors_bin();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("go.mod"), "module example\n").unwrap();
    std::fs::write(
        src.join("main.go"),
        r#"
package main

func main() {}
"#,
    )
    .unwrap();

    let first = Command::new(gors.as_path())
        .args([
            "build",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to run gors build: {e}"));
    assert!(
        first.status.success(),
        "initial gors build failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    let stale_file = out.join("stale.rs");
    std::fs::write(&stale_file, "stale").unwrap();
    let mut manifest = gors::compiler::manifest::BuildManifest::load(&out).unwrap();
    manifest.modules.insert(
        "stale.rs".to_string(),
        gors::compiler::manifest::ModuleEntry {
            content_hash: "stale".to_string(),
            output_file: "stale.rs".to_string(),
        },
    );
    manifest.save(&out).unwrap();

    let second = Command::new(gors.as_path())
        .args([
            "build",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap_or_else(|e| panic!("failed to rerun gors build: {e}"));
    assert!(
        second.status.success(),
        "second gors build failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        !stale_file.exists(),
        "stale manifest output should be removed"
    );
}

#[test]
fn run_programs_rust_backend() {
    let gors = gors_bin();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    let mut passed = 0;
    let mut failed: Vec<(String, String)> = Vec::new();

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_str().unwrap();

        let go_out = Command::new("go")
            .args(["run", "."])
            .current_dir(dir)
            .output();
        let go_stdout = match go_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => {
                eprintln!("Skipping {name} - go run failed");
                continue;
            }
        };

        let gors_out = Command::new(gors.as_path())
            .args(["run", dir.to_str().unwrap()])
            .output()
            .unwrap_or_else(|e| panic!("failed to run gors on {name}: {e}"));

        let gors_stdout = String::from_utf8_lossy(&gors_out.stdout);

        if gors_out.status.success() && gors_stdout == go_stdout.as_str() {
            passed += 1;
        } else if gors_out.status.success() {
            failed.push((
                name.to_string(),
                format!(
                    "Output mismatch:\nExpected: {:?}\nGot: {:?}",
                    go_stdout, gors_stdout
                ),
            ));
        } else {
            failed.push((
                name.to_string(),
                format!(
                    "gors run failed:\n{}",
                    String::from_utf8_lossy(&gors_out.stderr)
                ),
            ));
        }
    }

    eprintln!("\nResults: {passed}/{} passed", passed + failed.len());
    if !failed.is_empty() {
        for (name, err) in &failed {
            eprintln!("  FAIL {name}: {}", err.lines().next().unwrap_or(""));
        }
    }
    assert!(failed.is_empty(), "{} tests failed", failed.len());
}

#[test]
fn run_programs_go_runner() {
    let go_bin = go_runner_bin();
    let dirs = discover_program_dirs();
    assert!(
        !dirs.is_empty(),
        "No programs found in fixtures/go_programs"
    );

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_str().unwrap();

        let go_out = Command::new("go")
            .args(["run", "."])
            .current_dir(dir)
            .output();
        let go_stdout = match go_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => continue,
        };

        let runner_out = Command::new(go_bin.as_path())
            .args(["run", dir.to_str().unwrap()])
            .output()
            .unwrap_or_else(|e| panic!("Failed to run go runner on {name}: {e}"));

        if runner_out.status.success() {
            let actual = String::from_utf8_lossy(&runner_out.stdout);
            assert_eq!(
                actual.as_ref(),
                go_stdout.as_str(),
                "Output mismatch for {name}"
            );
        }
    }
}
