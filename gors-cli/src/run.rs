use crate::cache_paths::gors_cache_base;
use crate::options::Run;
use crate::program::{ProgramBuild, ProgramBuildRequest};
use crate::rustc::RustcProfile;
use crate::timings::TimingCollector;
use std::path::Path;

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
    let cache_base = gors_cache_base()?;
    let profile = RustcProfile::from_release_flag(cmd.release);
    let request = ProgramBuildRequest::new(&cache_base, &source_paths, cmd.jobs);
    let mut program = ProgramBuild::open(request, timings.clone())?;
    let executable = program.ensure_executable(profile)?;

    let execute_timer = timings.phase("cli.execute");
    let mut child = program.spawn_and_release(&executable, &program_args)?;
    let status = child.wait()?;
    drop(execute_timer);
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "run")?;
    std::process::exit(status.code().unwrap_or(1));
}
