use crate::cache::{
    CacheAccessLock, CacheRequest, CacheRequestOptions, CliCacheManifest, InputSnapshot,
    generated_file_hashes, maybe_prune_cli_cache,
};
use crate::cache_paths::{build_cache_dir, gors_cache_base};
use crate::compiler::{cli_workspace, compile_program};
use crate::diagnostics::print_compiler_error;
use crate::options::Build;
use crate::output::{OutputDirectoryLock, write_generated_output_locked, write_source_map};
use crate::timings::TimingCollector;
use std::path::{Path, PathBuf};

pub fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let cache_base = gors_cache_base()?;
    build_with_cache_base(cmd, &cache_base)
}

pub fn build_with_cache_base(
    cmd: Build,
    cache_base: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let timings = TimingCollector::new(cmd.jobs);
    let compiler_host = gors::compiler::CompilerHost::new(cmd.jobs)?;
    let source_paths = vec![cmd.path.clone()];
    let output_dir = cmd
        .output
        .as_deref()
        .map(PathBuf::from)
        .map_or_else(|| build_cache_dir(&cmd.path), Ok)?;
    let sourcemap_path = cmd.sourcemap.as_deref().map(PathBuf::from);
    let request = CacheRequest::new(CacheRequestOptions {
        command: "build",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&output_dir),
        sourcemap: sourcemap_path.as_deref(),
    })?;
    maybe_prune_cli_cache(cache_base, Some(&output_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(cache_base)?;
    let output_lock = OutputDirectoryLock::acquire(&output_dir)?;

    let source_load_timer = timings.phase("cli.source_load");
    let loaded = gors::workspace::load_program(cli_workspace()?, &cmd.path)?;
    let inputs = InputSnapshot::capture(&loaded)?;
    drop(source_load_timer);

    let cached_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&output_dir, &request, &inputs)
    };
    if let Some(manifest) = cached_manifest {
        timings.cache_event("compiler", true);
        println!(
            "Reused {} cached files from {}",
            manifest.generated_file_count(),
            output_dir.display()
        );
        timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build")?;
        drop(output_lock);
        drop(cache_access_lock);
        return Ok(());
    }
    timings.cache_event("compiler", false);

    let primary_file = loaded.primary_diagnostic_path().to_string();

    let compile_timer = timings.phase("cli.compile");
    let compilation = compile_program(
        loaded.into_input(),
        sourcemap_path.is_some(),
        &compiler_host,
    );
    timings.scheduler_telemetry(compiler_host.telemetry());
    drop(compiler_host);
    let (compiled, source_map_plan) = match compilation {
        Ok(compiled) => compiled,
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
    let stats = write_generated_output_locked(&output, &output_dir)?;
    let sourcemap = sourcemap_path
        .as_deref()
        .map(|path| {
            let plan = source_map_plan
                .as_ref()
                .ok_or("compiler did not return the requested source-map plan")?;
            write_source_map(&output, plan, path)
        })
        .transpose()?;
    drop(write_timer);

    let completed_request = CacheRequest::new(CacheRequestOptions {
        command: "build",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&output_dir),
        sourcemap: sourcemap_path.as_deref(),
    })?;
    if request == completed_request {
        CliCacheManifest::new(&request, inputs, generated_files, sourcemap).save(&output_dir)?;
    }

    let output_dir_display = output_dir.display();
    if stats.removed == 0 {
        println!(
            "Wrote {} files to {output_dir_display} ({} unchanged)",
            stats.written, stats.skipped
        );
    } else {
        println!(
            "Wrote {} files to {output_dir_display} ({} unchanged, {} removed)",
            stats.written, stats.skipped, stats.removed
        );
    }

    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build")?;
    drop(output_lock);
    drop(cache_access_lock);
    Ok(())
}
