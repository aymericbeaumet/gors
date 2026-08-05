use crate::cache_paths::gors_cache_base;
use crate::options::Run;
use crate::program::{ProgramBuild, ProgramBuildRequest};
use crate::rustc::RustcProfile;
use crate::timings::TimingCollector;
use std::path::Path;

pub fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let timings = TimingCollector::new(cmd.jobs);
    let cache_base = gors_cache_base()?;
    let profile = RustcProfile::from_release_flag(cmd.release);
    let request = ProgramBuildRequest::new(&cache_base, &cmd.paths, cmd.jobs);
    let mut program = ProgramBuild::open(request, timings.clone())?;
    let executable = program.ensure_executable(profile)?;

    let execute_timer = timings.phase("cli.execute");
    let mut child = program.spawn_and_release(&executable, &cmd.program_args)?;
    let status = child.wait()?;
    drop(execute_timer);
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "run")?;
    std::process::exit(status.code().unwrap_or(1));
}
