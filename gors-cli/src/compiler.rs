use gors::compiler::input::{InputError, ProgramInput, WorkspaceKey};

const CLI_WORKSPACE_KEY: &str = "gors-cli";

pub fn cli_workspace() -> Result<WorkspaceKey, InputError> {
    WorkspaceKey::ad_hoc(CLI_WORKSPACE_KEY)
}

pub fn compile_program(
    program: ProgramInput,
    source_maps: bool,
    host: &gors::compiler::CompilerHost,
) -> Result<
    (
        gors::compiler::CompiledProgram,
        Option<gors::compiler::SourceMapPlan>,
    ),
    gors::compiler::CompilerError,
> {
    let mut session = host.session(gors::compiler::db::BuildConfig::default())?;
    if source_maps {
        session
            .compile_program_with_source_map(program)
            .map(|(compiled, plan)| (compiled, Some(plan)))
    } else {
        session
            .compile_program(program)
            .map(|compiled| (compiled, None))
    }
}
