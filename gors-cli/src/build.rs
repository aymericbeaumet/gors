use crate::cache_paths::gors_cache_base;
use crate::options::Build;
use crate::program::{ProgramBuild, ProgramBuildRequest};
use crate::public_executable::{default_executable_path, publish_executable};
use crate::rustc::RustcProfile;
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
    let destination = if let Some(output) = cmd.output.as_deref() {
        PathBuf::from(output)
    } else {
        let first = cmd
            .paths
            .first()
            .ok_or("build requires at least one source selection")?;
        default_executable_path(Path::new(first))?
    };
    let timings = TimingCollector::new(cmd.jobs);
    let request = ProgramBuildRequest::new(cache_base, &cmd.paths, cmd.jobs);
    let mut program = ProgramBuild::open(request, timings.clone())?;
    let executable = program.ensure_executable(RustcProfile::Production)?;
    let published = publish_executable(&executable, &destination)?;

    println!("Wrote executable {}", published.path().display());
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build")
}
