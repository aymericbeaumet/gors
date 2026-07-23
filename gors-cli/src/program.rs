use crate::cache::{
    CacheAccessLock, CliCacheManifest, GeneratedRustIdentity, GeneratedRustIdentityOptions,
    InputSnapshot, generated_file_hashes, maybe_prune_cli_cache, refresh_runtime_selection,
};
use crate::cache_paths::program_cache_dir;
use crate::compiler::{cli_workspace, compile_program};
use crate::output::{FileWriteStats, OutputDirectoryLock, write_generated_output_locked};
use crate::runtime_link::{ResolvedRuntime, resolve_runtime};
use crate::rustc::{
    AdmittedRustc, ExecutableProduct, RustcAction, RustcProfile, compile_generated_binary,
};
use crate::timings::TimingCollector;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

pub struct ProgramBuildRequest<'a> {
    cache_base: &'a Path,
    source_paths: &'a [String],
    jobs: NonZeroUsize,
}

#[derive(Clone, Copy)]
pub enum SourceMapNeed<'a> {
    NotRequested,
    WriteTo(&'a Path),
}

#[derive(Clone, Debug)]
pub struct GeneratedProduct {
    directory: PathBuf,
    file_count: usize,
    cache_hit: bool,
    writes: Option<GeneratedWrites>,
}

#[derive(Clone, Copy, Debug)]
pub struct GeneratedWrites {
    pub written: usize,
    pub skipped: usize,
    pub removed: usize,
}

/// One locked program-cache transaction shared by source and executable users.
///
/// Opening loads one immutable source revision and performs only metadata-level
/// cache admission. Generated Rust and runtime/toolchain state are inspected
/// lazily by the corresponding `ensure_*` method.
pub struct ProgramBuild {
    cache_base: PathBuf,
    source_paths: Vec<String>,
    jobs: NonZeroUsize,
    identity: GeneratedRustIdentity,
    cache_dir: PathBuf,
    timings: TimingCollector,
    cache_access_lock: Option<CacheAccessLock>,
    output_lock: Option<OutputDirectoryLock>,
    loaded: Option<gors::workspace::LoadedProgram>,
    inputs: Option<InputSnapshot>,
    manifest: Option<CliCacheManifest>,
    runtime: Option<ResolvedRuntime>,
    generated: Option<GeneratedProduct>,
    compiler_cache_reported: bool,
}

impl<'a> ProgramBuildRequest<'a> {
    #[must_use]
    pub const fn new(cache_base: &'a Path, source_paths: &'a [String], jobs: NonZeroUsize) -> Self {
        Self {
            cache_base,
            source_paths,
            jobs,
        }
    }
}

impl ProgramBuild {
    pub fn open(
        request: ProgramBuildRequest<'_>,
        timings: TimingCollector,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
            source_paths: request.source_paths,
        })?;
        let cache_dir = program_cache_dir(request.cache_base, &identity);
        maybe_prune_cli_cache(request.cache_base, Some(&cache_dir))?;
        let cache_access_lock = CacheAccessLock::acquire_shared(request.cache_base)?;
        let output_lock = OutputDirectoryLock::acquire(&cache_dir)?;

        let source_load_timer = timings.phase("cli.source_load");
        let loaded = gors::workspace::load_program_files(cli_workspace()?, request.source_paths)?;
        let inputs = InputSnapshot::capture(&loaded)?;
        drop(source_load_timer);

        let manifest = {
            let _cache_timer = timings.phase("cli.cache_lookup");
            CliCacheManifest::load_if_source_revision_matches(&cache_dir, &identity, &inputs)
        };

        Ok(Self {
            cache_base: request.cache_base.to_path_buf(),
            source_paths: request.source_paths.to_vec(),
            jobs: request.jobs,
            identity,
            cache_dir,
            timings,
            cache_access_lock: Some(cache_access_lock),
            output_lock: Some(output_lock),
            loaded: Some(loaded),
            inputs: Some(inputs),
            manifest,
            runtime: None,
            generated: None,
            compiler_cache_reported: false,
        })
    }

    pub fn ensure_generated(
        &mut self,
        source_map: SourceMapNeed<'_>,
    ) -> Result<GeneratedProduct, Box<dyn std::error::Error>> {
        if matches!(source_map, SourceMapNeed::NotRequested)
            && let Some(generated) = &self.generated
        {
            return Ok(generated.clone());
        }

        let reusable_dependency = if let Some(manifest) = self.manifest.as_mut() {
            let generated_ready = manifest.generated_files_are_current(&self.cache_dir);
            let source_map_ready = match source_map {
                SourceMapNeed::NotRequested => true,
                SourceMapNeed::WriteTo(path) => manifest.reuse_sourcemap(path)?,
            };
            (generated_ready && source_map_ready)
                .then(|| manifest.runtime_dependency().ok())
                .flatten()
        } else {
            None
        };

        if let Some(dependency) = reusable_dependency {
            let runtime = resolve_runtime(&self.cache_base, dependency)?;
            let output = runtime.output_descriptor();
            let manifest = self
                .manifest
                .as_mut()
                .ok_or("generated manifest disappeared during cache admission")?;
            refresh_runtime_selection(
                &self.cache_dir,
                manifest,
                &output,
                runtime.rustc_path(),
                runtime.rustc_snapshot_identity(),
            )?;
            manifest.save(&self.cache_dir)?;
            let product = GeneratedProduct {
                directory: self.cache_dir.clone(),
                file_count: manifest.generated_file_count(),
                cache_hit: true,
                writes: None,
            };
            self.runtime = Some(runtime);
            self.generated = Some(product.clone());
            self.report_compiler_cache(true);
            return Ok(product);
        }

        self.manifest = None;
        let loaded = self
            .loaded
            .take()
            .ok_or("source revision is unavailable for generated-Rust compilation")?;
        let inputs = self
            .inputs
            .take()
            .ok_or("input snapshot is unavailable for generated-Rust compilation")?;
        let compiler_host = gors::compiler::CompilerHost::new(self.jobs)?;

        let compile_timer = self.timings.phase("cli.compile");
        let compilation = compile_program(
            loaded.into_input(),
            matches!(source_map, SourceMapNeed::WriteTo(_)),
            &compiler_host,
        );
        self.timings.scheduler_telemetry(compiler_host.telemetry());
        drop(compiler_host);
        let (compiled, source_map_plan) = compilation?;
        drop(compile_timer);

        let print_timer = self.timings.phase("cli.print");
        let output = gors::printer::generate_multi(compiled)?;
        drop(print_timer);
        let runtime = resolve_runtime(&self.cache_base, output.runtime.clone())?;
        let runtime_output = runtime.output_descriptor();
        let generated_files = generated_file_hashes(&output);
        let write_timer = self.timings.phase("cli.file_writes");
        let writes = write_generated_output_locked(&output, &self.cache_dir, &runtime_output)?;
        let source_map_artifact = match source_map {
            SourceMapNeed::NotRequested => None,
            SourceMapNeed::WriteTo(path) => {
                let plan = source_map_plan
                    .as_ref()
                    .ok_or("compiler did not return the requested source-map plan")?;
                Some(crate::output::write_source_map(&output, plan, path)?)
            }
        };
        drop(write_timer);

        let completed_identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
            source_paths: &self.source_paths,
        })?;
        if self.identity != completed_identity {
            return Err(
                "generated-Rust identity changed while compiling; rerun the command".into(),
            );
        }

        let mut manifest = CliCacheManifest::new(
            &self.identity,
            inputs,
            generated_files,
            source_map_artifact,
            &output.runtime,
        );
        manifest.refresh_runtime(
            &runtime_output,
            runtime.rustc_path(),
            runtime.rustc_snapshot_identity(),
        )?;
        manifest.save(&self.cache_dir)?;
        manifest.save_terminal(&self.cache_dir)?;
        let product = GeneratedProduct {
            directory: self.cache_dir.clone(),
            file_count: manifest.generated_file_count(),
            cache_hit: false,
            writes: Some(writes.into()),
        };
        self.manifest = Some(manifest);
        self.runtime = Some(runtime);
        self.generated = Some(product.clone());
        self.report_compiler_cache(false);
        Ok(product)
    }

    pub fn ensure_executable(
        &mut self,
        profile: RustcProfile,
    ) -> Result<ExecutableProduct, Box<dyn std::error::Error>> {
        let output_path = self.cache_dir.join(profile.executable_filename());
        if let Some(action) = self.action_from_manifest(profile, &output_path)?
            && let Some(executable) = self
                .manifest
                .as_ref()
                .and_then(|manifest| manifest.admit_executable(profile, &output_path, &action))
        {
            self.report_compiler_cache(true);
            self.timings.cache_event("rustc", true);
            return Ok(executable);
        }

        self.ensure_generated(SourceMapNeed::NotRequested)?;
        let action = self.current_action(profile, &output_path)?;
        if let Some(executable) = self
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.admit_executable(profile, &output_path, &action))
        {
            self.timings.cache_event("rustc", true);
            return Ok(executable);
        }

        self.timings.cache_event("rustc", false);
        let executable = compile_generated_binary(&action, &self.timings)?;
        let manifest = self
            .manifest
            .as_mut()
            .ok_or("generated manifest disappeared before executable publication")?;
        manifest.set_executable(profile, &executable, &action)?;
        // The executable is published first; its admitting terminal manifest
        // is the final commit point for the action.
        manifest.save_terminal(&self.cache_dir)?;
        Ok(executable)
    }

    pub fn spawn_and_release(
        mut self,
        executable: &ExecutableProduct,
        arguments: &[String],
    ) -> Result<Child, Box<dyn std::error::Error>> {
        let mut command = Command::new(executable.path());
        command.args(arguments);
        let lock = self
            .output_lock
            .take()
            .ok_or("program output lock was already released")?;
        let child = lock.spawn_and_release(&mut command)?;
        drop(self.cache_access_lock.take());
        Ok(child)
    }

    fn current_action(
        &self,
        profile: RustcProfile,
        output_path: &Path,
    ) -> Result<RustcAction, Box<dyn std::error::Error>> {
        let runtime = self
            .runtime
            .as_ref()
            .ok_or("runtime selection is unavailable for terminal compilation")?;
        let manifest = self
            .manifest
            .as_ref()
            .ok_or("generated manifest is unavailable for terminal compilation")?;
        Ok(RustcAction::for_generated_binary(
            &self.cache_dir,
            output_path,
            runtime.artifact_path(),
            runtime.descriptor(),
            AdmittedRustc::new(runtime.rustc_path(), runtime.rustc_snapshot_identity()),
            manifest.generated_files(),
            profile,
        )?)
    }

    fn action_from_manifest(
        &self,
        profile: RustcProfile,
        output_path: &Path,
    ) -> Result<Option<RustcAction>, Box<dyn std::error::Error>> {
        let Some(manifest) = self.manifest.as_ref() else {
            return Ok(None);
        };
        let (Some(runtime), Some(artifact_path), Some(rustc_path), Some(snapshot_identity)) = (
            manifest.selected_runtime(),
            manifest.selected_artifact_path(),
            manifest.selected_rustc_path(),
            manifest.selected_rustc_snapshot_identity(),
        ) else {
            return Ok(None);
        };
        Ok(Some(RustcAction::for_generated_binary(
            &self.cache_dir,
            output_path,
            artifact_path,
            runtime,
            AdmittedRustc::new(rustc_path, snapshot_identity),
            manifest.generated_files(),
            profile,
        )?))
    }

    fn report_compiler_cache(&mut self, hit: bool) {
        if !self.compiler_cache_reported {
            self.timings.cache_event("compiler", hit);
            self.compiler_cache_reported = true;
        }
    }
}

impl GeneratedProduct {
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    #[must_use]
    pub const fn file_count(&self) -> usize {
        self.file_count
    }

    #[must_use]
    pub const fn cache_hit(&self) -> bool {
        self.cache_hit
    }

    #[must_use]
    pub const fn writes(&self) -> Option<GeneratedWrites> {
        self.writes
    }
}

impl From<FileWriteStats> for GeneratedWrites {
    fn from(value: FileWriteStats) -> Self {
        Self {
            written: value.written,
            skipped: value.skipped,
            removed: value.removed,
        }
    }
}
