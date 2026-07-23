use crate::cache_paths::gors_cache_base;
use crate::options::EmitRust;
use crate::program::{ProgramBuild, ProgramBuildRequest, SourceMapNeed};
use crate::timings::TimingCollector;
use std::path::Path;

pub fn emit_rust(cmd: EmitRust) -> Result<(), Box<dyn std::error::Error>> {
    let cache_base = gors_cache_base()?;
    emit_rust_with_cache_base(cmd, &cache_base)
}

pub fn emit_rust_with_cache_base(
    cmd: EmitRust,
    cache_base: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let timings = TimingCollector::new(cmd.jobs);
    let request = ProgramBuildRequest::new(cache_base, &cmd.paths, cmd.jobs);
    let mut program = ProgramBuild::open(request, timings.clone())?;
    let source_map = cmd
        .sourcemap
        .as_deref()
        .map(Path::new)
        .map_or(SourceMapNeed::NotRequested, SourceMapNeed::WriteTo);
    program.ensure_generated(source_map)?;
    let writes = program.export_generated_rust(Path::new(&cmd.output))?;

    if writes.removed == 0 {
        println!(
            "Wrote {} Rust files to {} ({} unchanged)",
            writes.written, cmd.output, writes.skipped
        );
    } else {
        println!(
            "Wrote {} Rust files to {} ({} unchanged, {} removed)",
            writes.written, cmd.output, writes.skipped, writes.removed
        );
    }
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "emit-rust")
}
