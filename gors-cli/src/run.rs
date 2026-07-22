use crate::cache::{
    CacheAccessLock, CacheRequest, CacheRequestOptions, CliCacheManifest, InputSnapshot,
    generated_file_hashes, maybe_prune_cli_cache,
};
use crate::cache_paths::{gors_cache_base, run_cache_dir};
use crate::compiler::{cli_workspace, compile_program};
use crate::diagnostics::print_compiler_error;
use crate::options::Run;
use crate::output::{OutputDirectoryLock, write_generated_output_locked};
use crate::rustc::compile_generated_binary;
use crate::timings::TimingCollector;
use std::path::Path;
use std::process::Command;

/// Split CLI arguments into source paths and program arguments.
///
/// If the first argument ends with `.go`, all leading `.go` arguments are source
/// files. Otherwise, the first argument is a directory/package path. Everything
/// after the source paths is passed through to the compiled program.
pub fn split_run_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    if args.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if args
        .first()
        .is_some_and(|argument| argument.ends_with(".go"))
    {
        let split = args
            .iter()
            .position(|argument| !argument.ends_with(".go"))
            .unwrap_or(args.len());
        (
            args.get(..split).unwrap_or_default().to_vec(),
            args.get(split..).unwrap_or_default().to_vec(),
        )
    } else {
        (
            args.first().cloned().into_iter().collect(),
            args.get(1..).unwrap_or_default().to_vec(),
        )
    }
}

pub fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let (source_paths, program_args) = split_run_args(&cmd.args);
    let timings = TimingCollector::new(cmd.jobs);
    let compiler_host = gors::compiler::CompilerHost::new(cmd.jobs)?;
    let cache_base = gors_cache_base()?;
    let cache_dir = run_cache_dir(&source_paths, cmd.release)?;
    maybe_prune_cli_cache(&cache_base, Some(&cache_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base)?;
    let cache_lock = OutputDirectoryLock::acquire(&cache_dir)?;
    let request = CacheRequest::new(CacheRequestOptions {
        command: "run",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&cache_dir),
        sourcemap: None,
    })?;

    let source_load_timer = timings.phase("cli.source_load");
    let loaded = gors::workspace::load_program_files(cli_workspace()?, &source_paths)?;
    let inputs = InputSnapshot::capture(&loaded)?;
    drop(source_load_timer);

    let mut cache_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&cache_dir, &request, &inputs)
    };

    if cache_manifest.is_some() {
        timings.cache_event("compiler", true);
        drop(loaded);
        drop(inputs);
        drop(compiler_host);
    } else {
        timings.cache_event("compiler", false);
        let primary_file = loaded.primary_diagnostic_path().to_string();

        let compile_timer = timings.phase("cli.compile");
        let compilation = compile_program(loaded.into_input(), false, &compiler_host);
        timings.scheduler_telemetry(compiler_host.telemetry());
        drop(compiler_host);
        let compiled = match compilation {
            Ok((compiled, None)) => compiled,
            Ok((_, Some(_))) => {
                return Err("unexpected source-map plan for run compilation".into());
            }
            Err(err) => {
                print_compiler_error(&err, &primary_file);
                std::process::exit(1);
            }
        };
        drop(compile_timer);

        let print_timer = timings.phase("cli.print");
        let output = gors::printer::generate_multi(compiled)?;
        drop(print_timer);
        let generated_files = generated_file_hashes(&output);
        let write_timer = timings.phase("cli.file_writes");
        write_generated_output_locked(&output, &cache_dir)?;
        drop(write_timer);

        let completed_request = CacheRequest::new(CacheRequestOptions {
            command: "run",
            source_paths: &source_paths,
            release: cmd.release,
            output: Some(&cache_dir),
            sourcemap: None,
        })?;
        if request == completed_request {
            let manifest = CliCacheManifest::new(&request, inputs, generated_files, None);
            manifest.save(&cache_dir)?;
            cache_manifest = Some(manifest);
        }
    }

    let Some(mut cache_manifest) = cache_manifest else {
        return Err("source inputs changed while compiling; rerun the command".into());
    };
    let bin_path = cache_dir.join("main");
    if cache_manifest.executable_is_valid(&bin_path) {
        timings.cache_event("rustc", true);
    } else {
        timings.cache_event("rustc", false);
        compile_generated_binary(&cache_dir, &bin_path, cmd.release, &timings)?;
        cache_manifest.set_executable(&bin_path)?;
        cache_manifest.save(&cache_dir)?;
    }

    let execute_timer = timings.phase("cli.execute");
    let mut command = Command::new(&bin_path);
    command.args(&program_args);
    let mut child = cache_lock.spawn_and_release(&mut command)?;
    drop(cache_access_lock);
    let status = child.wait()?;
    drop(execute_timer);
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "run")?;
    std::process::exit(status.code().unwrap_or(1));
}
