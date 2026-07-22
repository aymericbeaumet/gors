#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const LOCK_CHILD_DIRECTORY_ENV: &str = "GORS_TEST_LOCK_CHILD_DIRECTORY";

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn timing_phase_names(report: &serde_json::Value) -> Vec<&str> {
    report
        .get("phases")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .iter()
        .map(|phase| {
            phase
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap()
        })
        .collect()
}

fn timing_report_version(report: &serde_json::Value) -> u64 {
    report
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap()
}

fn compiler_cache_hit(report: &serde_json::Value) -> bool {
    report
        .get("cacheEvents")
        .and_then(serde_json::Value::as_array)
        .and_then(|events| events.first())
        .and_then(|event| event.get("hit"))
        .and_then(serde_json::Value::as_bool)
        .unwrap()
}

#[test]
fn command_line_compilation_uses_an_explicit_stable_workspace_identity() {
    assert_eq!(
        cli_workspace().unwrap(),
        WorkspaceKey::AdHoc("gors-cli".into())
    );
}

#[test]
fn build_timing_v5_reports_single_load_before_hit_or_miss_cache_lookup() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("main.go");
    let output = temporary.path().join("output");
    let cache = temporary.path().join("cache");
    let miss_timings = temporary.path().join("miss.json");
    let hit_timings = temporary.path().join("hit.json");
    std::fs::write(
        &source,
        "package main\n\nfunc main() { println(\"timing\") }\n",
    )
    .unwrap();

    let command = |timings: &Path| Build {
        path: source.to_string_lossy().into_owned(),
        release: false,
        sourcemap: None,
        output: Some(output.to_string_lossy().into_owned()),
        timings_json: Some(timings.to_string_lossy().into_owned()),
        jobs: NonZeroUsize::MIN,
    };
    build_with_cache_base(command(&miss_timings), &cache).unwrap();
    build_with_cache_base(command(&hit_timings), &cache).unwrap();

    let read_report = |path: &Path| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };

    let miss = read_report(&miss_timings);
    assert_eq!(timing_report_version(&miss), 5);
    assert_eq!(
        timing_phase_names(&miss),
        [
            "cli.source_load",
            "cli.cache_lookup",
            "cli.compile",
            "cli.print",
            "cli.file_writes",
        ]
    );
    assert!(!compiler_cache_hit(&miss));

    let hit = read_report(&hit_timings);
    assert_eq!(timing_report_version(&hit), 5);
    assert_eq!(
        timing_phase_names(&hit),
        ["cli.source_load", "cli.cache_lookup"]
    );
    assert!(compiler_cache_hit(&hit));
}

#[test]
fn split_run_args_keeps_single_file_as_source() {
    let (sources, program_args) = split_run_args(&args(&["main.go", "--", "arg"]));
    assert_eq!(sources, args(&["main.go"]));
    assert_eq!(program_args, args(&["--", "arg"]));
}

#[test]
fn split_run_args_groups_leading_go_files() {
    let (sources, program_args) = split_run_args(&args(&["main.go", "helpers.go", "--flag"]));
    assert_eq!(sources, args(&["main.go", "helpers.go"]));
    assert_eq!(program_args, args(&["--flag"]));
}

#[test]
fn split_run_args_treats_directory_as_single_source() {
    let (sources, program_args) = split_run_args(&args(&[".", "--flag", "value"]));
    assert_eq!(sources, args(&["."]));
    assert_eq!(program_args, args(&["--flag", "value"]));
}

#[test]
fn split_run_args_treats_package_path_as_single_source() {
    let (sources, program_args) = split_run_args(&args(&["./cmd/myapp", "arg"]));
    assert_eq!(sources, args(&["./cmd/myapp"]));
    assert_eq!(program_args, args(&["arg"]));
}

#[test]
fn build_accepts_timing_report_option() {
    let opts = Opts::try_parse_from([
        "gors",
        "build",
        "--jobs",
        "3",
        "--timings-json",
        "timings.json",
        "main.go",
    ])
    .unwrap();
    let SubCommand::Build(build) = opts.subcmd else {
        panic!("expected build command");
    };
    assert_eq!(build.timings_json.as_deref(), Some("timings.json"));
    assert_eq!(build.jobs.get(), 3);
    assert_eq!(build.path, "main.go");
}

#[test]
fn run_accepts_timing_option_before_trailing_program_arguments() {
    let opts = Opts::try_parse_from([
        "gors",
        "run",
        "--jobs",
        "2",
        "--timings-json",
        "timings.json",
        "main.go",
        "--program-flag",
    ])
    .unwrap();
    let SubCommand::Run(run) = opts.subcmd else {
        panic!("expected run command");
    };
    assert_eq!(run.timings_json.as_deref(), Some("timings.json"));
    assert_eq!(run.jobs.get(), 2);
    assert_eq!(run.args, args(&["main.go", "--program-flag"]));
}

#[test]
fn compiler_job_budget_must_be_positive() {
    assert!(Opts::try_parse_from(["gors", "build", "--jobs", "0", "main.go"]).is_err());
}

#[test]
fn rustc_arguments_do_not_create_per_invocation_incremental_state() {
    let flags = Vec::from(RustcArgs {
        src: "main.rs",
        out: Some("main"),
        emit: None,
        release: false,
    });
    assert!(
        flags.iter().all(|flag| !flag.contains("incremental")),
        "{flags:?}"
    );
}

#[test]
fn write_generated_output_removes_files_missing_from_new_manifest() {
    let tmp = tempfile::tempdir().unwrap();

    let mut first_files = BTreeMap::new();
    first_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
    first_files.insert("stale.rs".to_string(), "fn stale() {}\n".to_string());
    let first = gors::printer::GeneratedOutput { files: first_files };
    let first_stats = write_generated_output(&first, tmp.path()).unwrap();
    assert_eq!(first_stats.written, 2);
    assert!(tmp.path().join("stale.rs").exists());

    let mut second_files = BTreeMap::new();
    second_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
    let second = gors::printer::GeneratedOutput {
        files: second_files,
    };
    let second_stats = write_generated_output(&second, tmp.path()).unwrap();

    assert_eq!(second_stats.skipped, 1);
    assert_eq!(second_stats.removed, 1);
    assert!(!tmp.path().join("stale.rs").exists());
}

#[test]
fn concurrent_output_publications_publish_one_consistent_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let output_dir = tmp.path().to_path_buf();
    let source_map_path = output_dir.join("program.map");
    let executable_path = output_dir.join("main");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let active_publishers = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let handles: Vec<_> = [("alpha", "fn alpha() {}\n"), ("beta", "fn beta() {}\n")]
        .into_iter()
        .map(|(module, body)| {
            let output_dir = output_dir.clone();
            let source_map_path = source_map_path.clone();
            let executable_path = executable_path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            let active_publishers = std::sync::Arc::clone(&active_publishers);
            std::thread::spawn(move || {
                let source_path = output_dir.join(format!("{module}.go"));
                std::fs::write(
                    &source_path,
                    format!("package main\n\nfunc main() {{ println(\"{module}\") }}\n"),
                )
                .unwrap();
                let source_paths = vec![source_path.to_string_lossy().into_owned()];
                let loaded = gors::workspace::load_program(
                    cli_workspace().unwrap(),
                    source_paths.first().expect("single source path"),
                )
                .unwrap();
                let inputs = InputSnapshot::capture(&loaded).unwrap();
                let request = CacheRequest::new(CacheRequestOptions {
                    command: "run",
                    source_paths: &source_paths,
                    release: false,
                    output: Some(&output_dir),
                    sourcemap: Some(&source_map_path),
                })
                .unwrap();
                let mut files = BTreeMap::new();
                files.insert(
                    "main.rs".to_string(),
                    format!("mod {module};\nfn main() {{ {module}(); }}\n"),
                );
                files.insert(format!("{module}.rs"), body.to_string());
                let output = gors::printer::GeneratedOutput { files };
                let generated_files = generated_file_hashes(&output);
                barrier.wait();

                let _output_lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
                assert_eq!(
                    active_publishers.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                    0,
                    "publication transactions overlapped"
                );
                std::thread::sleep(Duration::from_millis(25));
                write_generated_output_locked(&output, &output_dir).unwrap();
                prepare_atomic_write(&source_map_path, &format!("{module}-map\n"))
                    .unwrap()
                    .persist(&source_map_path)
                    .unwrap();
                let source_map = FileArtifact::capture(&source_map_path).unwrap();
                let mut manifest =
                    CliCacheManifest::new(&request, inputs, generated_files, Some(source_map));
                manifest.save(&output_dir).unwrap();
                prepare_atomic_write(&executable_path, &format!("{module}-executable\n"))
                    .unwrap()
                    .persist(&executable_path)
                    .unwrap();
                manifest.set_executable(&executable_path).unwrap();
                manifest.save(&output_dir).unwrap();
                assert_eq!(
                    active_publishers.fetch_sub(1, std::sync::atomic::Ordering::SeqCst),
                    1
                );
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let manifest = GeneratedOutputManifest::load(&output_dir).expect("manifest");
    let main = std::fs::read_to_string(output_dir.join("main.rs")).unwrap();
    let published = if manifest.contains("alpha.rs") {
        "alpha"
    } else {
        "beta"
    };
    let stale = if published == "alpha" {
        "beta"
    } else {
        "alpha"
    };

    assert!(main.contains(published));
    assert!(output_dir.join(format!("{published}.rs")).exists());
    assert!(!manifest.contains(&format!("{stale}.rs")));
    assert!(!output_dir.join(format!("{stale}.rs")).exists());
    assert_eq!(
        std::fs::read_to_string(&source_map_path).unwrap(),
        format!("{published}-map\n")
    );
    assert_eq!(
        std::fs::read_to_string(&executable_path).unwrap(),
        format!("{published}-executable\n")
    );

    let source_paths = vec![
        output_dir
            .join(format!("{published}.go"))
            .to_string_lossy()
            .into_owned(),
    ];
    let request = CacheRequest::new(CacheRequestOptions {
        command: "run",
        source_paths: &source_paths,
        release: false,
        output: Some(&output_dir),
        sourcemap: Some(&source_map_path),
    })
    .unwrap();
    let loaded = gors::workspace::load_program(
        cli_workspace().unwrap(),
        source_paths.first().expect("published source path"),
    )
    .unwrap();
    let inputs = InputSnapshot::capture(&loaded).unwrap();
    let cli_manifest = CliCacheManifest::load_if_generated_valid(&output_dir, &request, &inputs)
        .expect("published CLI cache manifest");
    assert!(cli_manifest.executable_is_valid(&executable_path));
}

#[test]
fn spawned_program_does_not_retain_publication_locks_while_running() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_base = tmp.path().join("cache");
    let output_dir = cache_base.join("run").join("entry");
    let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base).unwrap();
    let output_lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "tests::output_lock_child_waits_for_release",
            "--exact",
            "--nocapture",
        ])
        .env(LOCK_CHILD_DIRECTORY_ENV, tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = output_lock.spawn_and_release(&mut command).unwrap();
    drop(cache_access_lock);
    let started = tmp.path().join("child-started");
    let release = tmp.path().join("child-release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !started.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(started.is_file(), "child process did not start");
    assert!(
        child.try_wait().unwrap().is_none(),
        "child process exited before the lock check"
    );

    let (acquired_tx, acquired_rx) = mpsc::channel();
    let contender = std::thread::spawn(move || {
        let _lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
        acquired_tx.send(()).unwrap();
    });
    let acquired_while_running = acquired_rx.recv_timeout(Duration::from_secs(2)).is_ok();

    let (pruned_tx, pruned_rx) = mpsc::channel();
    let pruner = std::thread::spawn(move || {
        maybe_prune_cli_cache(&cache_base, None).unwrap();
        pruned_tx.send(()).unwrap();
    });
    let pruned_while_running = pruned_rx.recv_timeout(Duration::from_secs(2)).is_ok();

    std::fs::write(release, b"release").unwrap();
    let status = child.wait().unwrap();
    contender.join().unwrap();
    pruner.join().unwrap();

    assert!(
        acquired_while_running,
        "output lock remained held for the child process lifetime"
    );
    assert!(
        pruned_while_running,
        "cache-wide shared lock remained held for the child process lifetime"
    );
    assert!(status.success());
}

#[test]
fn output_lock_child_waits_for_release() {
    let Some(directory) = std::env::var_os(LOCK_CHILD_DIRECTORY_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::write(directory.join("child-started"), b"started").unwrap();
    let release = directory.join("child-release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !release.is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(release.is_file(), "parent did not release child process");
}
