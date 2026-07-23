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

fn terminal_toolchain() -> TerminalToolchain {
    let path = std::env::current_exe().unwrap();
    let target_libdir = path.parent().unwrap();
    TerminalToolchain::for_test(&path, &path, target_libdir, "test-target").unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
fn emit_rust_timing_v5_reports_single_load_before_hit_or_miss_cache_lookup() {
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

    let command = |timings: &Path| EmitRust {
        paths: vec![source.to_string_lossy().into_owned()],
        sourcemap: None,
        output: output.to_string_lossy().into_owned(),
        timings_json: Some(timings.to_string_lossy().into_owned()),
        jobs: NonZeroUsize::MIN,
    };
    emit_rust_with_cache_base(command(&miss_timings), &cache).unwrap();
    emit_rust_with_cache_base(command(&hit_timings), &cache).unwrap();

    assert!(output.join("main.rs").is_file());
    assert!(!output.join("__gors_runtime.rs").exists());
    assert!(!cache.join("runtime").exists());
    assert!(!cache.join("toolchains").exists());

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
fn generated_cache_rejects_go_mod_change_after_session_open() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("main.go");
    let module = temporary.path().join("go.mod");
    let cache = temporary.path().join("cache");
    std::fs::write(&source, "package main\n\nfunc main() { println(1) }\n").unwrap();
    std::fs::write(&module, "module example.com/first\n").unwrap();
    let paths = vec![source.to_string_lossy().into_owned()];
    let request = program::ProgramBuildRequest::new(&cache, &paths, NonZeroUsize::MIN);
    let mut initial =
        program::ProgramBuild::open(request, timings::TimingCollector::new(NonZeroUsize::MIN))
            .unwrap();
    initial
        .ensure_generated(program::SourceMapNeed::NotRequested)
        .unwrap();
    drop(initial);

    let request = program::ProgramBuildRequest::new(&cache, &paths, NonZeroUsize::MIN);
    let mut reopened =
        program::ProgramBuild::open(request, timings::TimingCollector::new(NonZeroUsize::MIN))
            .unwrap();
    std::fs::write(&module, "module example.com/changed\n").unwrap();

    let error = reopened
        .ensure_generated(program::SourceMapNeed::NotRequested)
        .unwrap_err();
    assert!(error.to_string().contains("identity changed"));
}

#[test]
fn build_publishes_runnable_production_and_reuses_it_across_outputs() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("program.go");
    let cache = temporary.path().join("cache");
    let first_output = temporary.path().join("first-program");
    let second_output = temporary.path().join("second-program");
    std::fs::write(
        &source,
        "package main\n\nfunc twice(value int) int { return value * 2 }\n\nfunc main() { println(twice(4)) }\n",
    )
    .unwrap();
    let command = |output: &Path| Build {
        paths: vec![source.to_string_lossy().into_owned()],
        output: Some(output.to_string_lossy().into_owned()),
        timings_json: None,
        jobs: NonZeroUsize::MIN,
    };
    let runtime_before = runtime_link::resolution_count();
    let rustc_before = rustc::compilation_count();

    build_with_cache_base(command(&first_output), &cache).unwrap();

    assert_eq!(runtime_link::resolution_count(), runtime_before + 1);
    assert_eq!(rustc::compilation_count(), rustc_before + 1);
    let execution = Command::new(&first_output).output().unwrap();
    assert!(execution.status.success());
    assert!(
        execution.stdout == b"8\n" || execution.stderr == b"8\n",
        "stdout={:?}, stderr={:?}",
        execution.stdout,
        execution.stderr
    );
    let first_bytes = std::fs::read(&first_output).unwrap();
    #[cfg(unix)]
    let first_inode = {
        use std::os::unix::fs::MetadataExt as _;
        std::fs::metadata(&first_output).unwrap().ino()
    };

    build_with_cache_base(command(&first_output), &cache).unwrap();

    assert_eq!(runtime_link::resolution_count(), runtime_before + 1);
    assert_eq!(rustc::compilation_count(), rustc_before + 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        assert_eq!(std::fs::metadata(&first_output).unwrap().ino(), first_inode);
    }

    std::fs::remove_dir_all(cache.join("runtime")).unwrap();
    build_with_cache_base(command(&second_output), &cache).unwrap();

    assert_eq!(runtime_link::resolution_count(), runtime_before + 1);
    assert_eq!(rustc::compilation_count(), rustc_before + 1);
    assert_eq!(std::fs::read(&second_output).unwrap(), first_bytes);

    let paths = vec![source.to_string_lossy().into_owned()];
    let request = program::ProgramBuildRequest::new(&cache, &paths, NonZeroUsize::MIN);
    let mut shared =
        program::ProgramBuild::open(request, timings::TimingCollector::new(NonZeroUsize::MIN))
            .unwrap();
    let production = shared.ensure_executable(RustcProfile::Production).unwrap();
    assert_eq!(std::fs::read(production.path()).unwrap(), first_bytes);
    assert_eq!(runtime_link::resolution_count(), runtime_before + 1);
    assert_eq!(rustc::compilation_count(), rustc_before + 1);
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
    assert_eq!(build.paths, args(&["main.go"]));
}

#[test]
fn run_requires_a_literal_delimiter_before_program_arguments() {
    let opts = Opts::try_parse_from([
        "gors",
        "run",
        "--jobs",
        "2",
        "--timings-json",
        "timings.json",
        "main.go",
        "helpers.go",
        "--",
        "--program-flag",
    ])
    .unwrap();
    let SubCommand::Run(run) = opts.subcmd else {
        panic!("expected run command");
    };
    assert_eq!(run.timings_json.as_deref(), Some("timings.json"));
    assert_eq!(run.jobs.get(), 2);
    assert_eq!(run.paths, args(&["main.go", "helpers.go"]));
    assert_eq!(run.program_args, args(&["--program-flag"]));
    assert!(Opts::try_parse_from(["gors", "run", "main.go", "--program-flag"]).is_err());
}

#[test]
fn emit_rust_requires_an_explicit_output_directory() {
    assert!(Opts::try_parse_from(["gors", "emit-rust", "main.go"]).is_err());
    let opts = Opts::try_parse_from(["gors", "emit-rust", "-o", "generated", "main.go"]).unwrap();
    let SubCommand::EmitRust(command) = opts.subcmd else {
        panic!("expected emit-rust command");
    };
    assert_eq!(command.paths, args(&["main.go"]));
    assert_eq!(command.output, "generated");
}

#[test]
fn compiler_job_budget_must_be_positive() {
    assert!(Opts::try_parse_from(["gors", "build", "--jobs", "0", "main.go"]).is_err());
}

#[test]
fn rustc_arguments_do_not_create_per_invocation_incremental_state() {
    let directory = tempfile::tempdir().unwrap();
    let generated_source = "fn main() {}\n";
    let generated_files = BTreeMap::from([(
        "main.rs".to_string(),
        sha256_hex(generated_source.as_bytes()),
    )]);
    std::fs::write(directory.path().join("main.rs"), generated_source).unwrap();
    let runtime = directory.path().join("lib__gors_runtime.rlib");
    let toolchain = terminal_toolchain();
    let action = RustcAction::for_generated_binary(
        directory.path(),
        &directory.path().join("main"),
        &runtime,
        &runtime_descriptor::test_runtime_link_descriptor(),
        &toolchain,
        &generated_files,
        RustcProfile::Development,
    )
    .unwrap();
    let flags = action
        .argv()
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        flags.iter().all(|flag| !flag.contains("incremental")),
        "{flags:?}"
    );
    assert_eq!(
        flags
            .iter()
            .filter(|flag| flag.as_str() == "--extern")
            .count(),
        1
    );
    assert!(
        flags
            .iter()
            .any(|flag| flag.starts_with("__gors_runtime=") && flag.ends_with(".rlib"))
    );
}

#[test]
fn write_generated_output_removes_files_missing_from_new_manifest() {
    let tmp = tempfile::tempdir().unwrap();

    let mut first_files = BTreeMap::new();
    first_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
    first_files.insert("stale.rs".to_string(), "fn stale() {}\n".to_string());
    let first = gors::printer::GeneratedOutput {
        files: first_files,
        runtime: runtime_descriptor::test_runtime_dependency(),
    };
    let first_stats = write_generated_output(&first, tmp.path()).unwrap();
    assert_eq!(first_stats.written, 2);
    assert!(tmp.path().join("stale.rs").exists());

    let mut second_files = BTreeMap::new();
    second_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
    let second = gors::printer::GeneratedOutput {
        files: second_files,
        runtime: runtime_descriptor::test_runtime_dependency(),
    };
    let second_stats = write_generated_output(&second, tmp.path()).unwrap();

    assert_eq!(second_stats.skipped, 1);
    assert_eq!(second_stats.removed, 1);
    assert!(!tmp.path().join("stale.rs").exists());
}

#[test]
fn generated_output_repairs_corrupt_missing_and_untracked_rust_files() {
    let temporary = tempfile::tempdir().unwrap();
    let output = gors::printer::GeneratedOutput {
        files: BTreeMap::from([
            ("main.rs".to_string(), "fn main() {}\n".to_string()),
            ("module.rs".to_string(), "pub fn value() {}\n".to_string()),
        ]),
        runtime: runtime_descriptor::test_runtime_dependency(),
    };
    write_generated_output(&output, temporary.path()).unwrap();
    std::fs::write(temporary.path().join("main.rs"), "corrupt\n").unwrap();
    std::fs::remove_file(temporary.path().join("module.rs")).unwrap();
    std::fs::write(temporary.path().join("untracked.rs"), "stale\n").unwrap();

    let stats = write_generated_output(&output, temporary.path()).unwrap();

    assert_eq!(stats.written, 2);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.removed, 1);
    assert_eq!(
        std::fs::read_to_string(temporary.path().join("main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert_eq!(
        std::fs::read_to_string(temporary.path().join("module.rs")).unwrap(),
        "pub fn value() {}\n"
    );
    assert!(!temporary.path().join("untracked.rs").exists());
}

#[test]
fn rust_export_removes_only_unchanged_files_owned_by_its_previous_manifest() {
    let temporary = tempfile::tempdir().unwrap();
    let first = gors::printer::GeneratedOutput {
        files: BTreeMap::from([
            ("main.rs".to_string(), "fn main() {}\n".to_string()),
            ("stale.rs".to_string(), "fn stale() {}\n".to_string()),
        ]),
        runtime: runtime_descriptor::test_runtime_dependency(),
    };
    write_generated_export(&first, temporary.path()).unwrap();
    std::fs::write(
        temporary.path().join("unrelated.rs"),
        "fn belongs_to_the_user() {}\n",
    )
    .unwrap();
    let second = gors::printer::GeneratedOutput {
        files: BTreeMap::from([("main.rs".to_string(), "fn main() {}\n".to_string())]),
        runtime: runtime_descriptor::test_runtime_dependency(),
    };

    let stats = write_generated_export(&second, temporary.path()).unwrap();

    assert_eq!(stats.removed, 1);
    assert!(!temporary.path().join("stale.rs").exists());
    assert_eq!(
        std::fs::read_to_string(temporary.path().join("unrelated.rs")).unwrap(),
        "fn belongs_to_the_user() {}\n"
    );
}

#[test]
fn generated_output_retains_only_target_neutral_runtime_dependency() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("__gors_runtime.rs"),
        "// stale source-bundled runtime\n",
    )
    .unwrap();
    let output = gors::printer::GeneratedOutput {
        files: BTreeMap::from([("main.rs".to_string(), "fn main() {}\n".to_string())]),
        runtime: runtime_descriptor::test_runtime_dependency(),
    };
    let stats = write_generated_output(&output, tmp.path()).unwrap();

    assert_eq!(stats.removed, 1);
    assert!(!tmp.path().join("__gors_runtime.rs").exists());
    let manifest = GeneratedOutputManifest::load(tmp.path()).unwrap();
    assert_eq!(
        manifest.runtime_dependency().unwrap(),
        runtime_descriptor::test_runtime_dependency()
    );
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
                let identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
                    source_paths: &source_paths,
                })
                .unwrap();
                let mut files = BTreeMap::new();
                files.insert(
                    "main.rs".to_string(),
                    format!("mod {module};\nfn main() {{ {module}(); }}\n"),
                );
                files.insert(format!("{module}.rs"), body.to_string());
                let runtime = runtime_descriptor::test_runtime_link_descriptor();
                let runtime_artifact = output_dir.join("runtime.rlib");
                let toolchain = terminal_toolchain();
                let output = gors::printer::GeneratedOutput {
                    files,
                    runtime: runtime_descriptor::test_runtime_dependency(),
                };
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
                let mut manifest = CliCacheManifest::new(
                    &identity,
                    inputs,
                    generated_files,
                    Some(source_map),
                    &output.runtime,
                );
                manifest
                    .refresh_runtime(&runtime, &runtime_artifact, &toolchain)
                    .unwrap();
                manifest.save(&output_dir).unwrap();
                prepare_atomic_write(&executable_path, &format!("{module}-executable\n"))
                    .unwrap()
                    .persist(&executable_path)
                    .unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    std::fs::set_permissions(
                        &executable_path,
                        std::fs::Permissions::from_mode(0o755),
                    )
                    .unwrap();
                }
                let action = RustcAction::for_generated_binary(
                    &output_dir,
                    &executable_path,
                    &runtime_artifact,
                    &runtime,
                    &toolchain,
                    manifest.generated_files(),
                    RustcProfile::Development,
                )
                .unwrap();
                let executable = ExecutableProduct::admit(&executable_path).unwrap();
                manifest
                    .set_executable(RustcProfile::Development, &executable, &action)
                    .unwrap();
                manifest.save_terminal(&output_dir).unwrap();
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
    let identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
        source_paths: &source_paths,
    })
    .unwrap();
    let loaded = gors::workspace::load_program(
        cli_workspace().unwrap(),
        source_paths.first().expect("published source path"),
    )
    .unwrap();
    let inputs = InputSnapshot::capture(&loaded).unwrap();
    let cli_manifest = CliCacheManifest::load_if_generated_valid(&output_dir, &identity, &inputs)
        .expect("published CLI cache manifest");
    let runtime = cli_manifest.runtime().expect("published runtime").clone();
    let artifact_path = cli_manifest
        .selected_artifact_path()
        .expect("published runtime artifact");
    let toolchain = cli_manifest
        .selected_terminal_toolchain()
        .expect("published terminal toolchain");
    let action = RustcAction::for_generated_binary(
        &output_dir,
        &executable_path,
        artifact_path,
        &runtime,
        toolchain,
        cli_manifest.generated_files(),
        RustcProfile::Development,
    )
    .unwrap();
    assert!(
        cli_manifest
            .admit_executable(RustcProfile::Development, &executable_path, &action)
            .is_some()
    );
}

#[test]
fn spawned_program_does_not_retain_publication_locks_while_running() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_base = tmp.path().join("cache");
    let output_dir = cache_base.join("programs").join("entry");
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
