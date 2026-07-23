use crate::cache::{
    CacheAccessLock, CliCacheManifest, GeneratedRustIdentity, GeneratedRustIdentityOptions,
    InputSnapshot, generated_file_hashes, maybe_prune_cli_cache, refresh_runtime_selection,
};
use crate::cache_paths::{gors_cache_base, program_cache_dir};
use crate::compiler::{cli_workspace, compile_program};
use crate::diagnostics::print_compiler_error;
use crate::options::Build;
use crate::output::{OutputDirectoryLock, write_generated_output_locked, write_source_map};
use crate::program::{ProgramBuild, ProgramBuildRequest, SourceMapNeed};
use crate::runtime_link::resolve_runtime;
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
    if cmd.output.is_none() {
        return build_cached_program(cmd, cache_base);
    }

    let timings = TimingCollector::new(cmd.jobs);
    let source_paths = vec![cmd.path.clone()];
    let identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
        source_paths: &source_paths,
    })?;
    let output_dir = cmd
        .output
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| program_cache_dir(cache_base, &identity));
    let sourcemap_path = cmd.sourcemap.as_deref().map(PathBuf::from);
    maybe_prune_cli_cache(cache_base, Some(&output_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(cache_base)?;
    let output_lock = OutputDirectoryLock::acquire(&output_dir)?;

    let source_load_timer = timings.phase("cli.source_load");
    let loaded = gors::workspace::load_program(cli_workspace()?, &cmd.path)?;
    let inputs = InputSnapshot::capture(&loaded)?;
    drop(source_load_timer);

    let cached_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&output_dir, &identity, &inputs)
    };
    if let Some(mut manifest) = cached_manifest {
        let source_map_ready = sourcemap_path
            .as_deref()
            .map(|path| manifest.reuse_sourcemap(path))
            .transpose()?
            .unwrap_or(true);
        // A stale dependency schema or missing requested presentation artifact
        // is a miss. Provider selection itself is terminal-only.
        if source_map_ready && let Ok(dependency) = manifest.runtime_dependency() {
            let runtime = resolve_runtime(cache_base, dependency)?;
            let runtime_output = runtime.output_descriptor();
            refresh_runtime_selection(
                &output_dir,
                &mut manifest,
                &runtime_output,
                runtime.rustc_path(),
                runtime.rustc_snapshot_identity(),
            )?;
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
    }
    timings.cache_event("compiler", false);

    let compiler_host = gors::compiler::CompilerHost::new(cmd.jobs)?;
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
    let runtime = resolve_runtime(cache_base, output.runtime.clone())?;
    let runtime_output = runtime.output_descriptor();
    let generated_files = generated_file_hashes(&output);
    let write_timer = timings.phase("cli.file_writes");
    let stats = write_generated_output_locked(&output, &output_dir, &runtime_output)?;
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

    let completed_identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
        source_paths: &source_paths,
    })?;
    if identity == completed_identity {
        let mut manifest = CliCacheManifest::new(
            &identity,
            inputs,
            generated_files,
            sourcemap,
            &output.runtime,
        );
        manifest.refresh_runtime(
            &runtime_output,
            runtime.rustc_path(),
            runtime.rustc_snapshot_identity(),
        )?;
        manifest.save(&output_dir)?;
        manifest.save_terminal(&output_dir)?;
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

fn build_cached_program(cmd: Build, cache_base: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let timings = TimingCollector::new(cmd.jobs);
    let source_paths = vec![cmd.path];
    let request = ProgramBuildRequest::new(cache_base, &source_paths, cmd.jobs);
    let mut program = ProgramBuild::open(request, timings.clone())?;
    let source_map = cmd
        .sourcemap
        .as_deref()
        .map(Path::new)
        .map_or(SourceMapNeed::NotRequested, SourceMapNeed::WriteTo);
    let generated = program.ensure_generated(source_map)?;

    if generated.cache_hit() {
        println!(
            "Reused {} cached files from {}",
            generated.file_count(),
            generated.directory().display()
        );
    } else if let Some(writes) = generated.writes() {
        let output = generated.directory().display();
        if writes.removed == 0 {
            println!(
                "Wrote {} files to {output} ({} unchanged)",
                writes.written, writes.skipped
            );
        } else {
            println!(
                "Wrote {} files to {output} ({} unchanged, {} removed)",
                writes.written, writes.skipped, writes.removed
            );
        }
    }

    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build")
}
