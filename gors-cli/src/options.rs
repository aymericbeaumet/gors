use clap::Parser;
use std::num::NonZeroUsize;

fn default_job_budget() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

const ROOT_HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
{usage-heading} {usage}

\x1b[1mCommands:\x1b[0m
{subcommands}

\x1b[1mAdvanced Commands:\x1b[0m
  \x1b[1mast\x1b[0m     Parse the named Go file and print the AST
  \x1b[1mtokens\x1b[0m  Scan the named Go file and print the tokens

\x1b[1mOptions:\x1b[0m
{options}{after-help}";

#[derive(Parser)]
#[command(
    name = "gors",
    author = "Aymeric Beaumet <hi@aymericbeaumet.com>",
    about = "gors is a Go-to-Rust compiler",
    disable_help_subcommand = true,
    disable_version_flag = true,
    help_template = ROOT_HELP_TEMPLATE
)]
pub struct Opts {
    #[command(subcommand)]
    pub subcmd: SubCommand,
}

#[derive(Parser)]
pub enum SubCommand {
    /// Parse the named Go file and print the AST
    #[command(hide = true)]
    Ast(Ast),
    /// Compile Go source into an optimized runnable executable
    #[command(display_order = 0)]
    Build(Build),
    /// Emit generated Rust source for inspection
    #[command(display_order = 0)]
    EmitRust(EmitRust),
    /// Print this message or the help of the given command(s)
    #[command(display_order = 0)]
    Help(Help),
    /// Compile and run Go source
    #[command(display_order = 0)]
    Run(Run),
    /// Scan the named Go file and print the tokens
    #[command(hide = true)]
    Tokens(Tokens),
    /// Print gors version
    #[command(display_order = 0)]
    Version,
}

#[derive(Parser)]
pub struct Ast {
    /// The files to parse
    #[arg(required = true)]
    pub files: Vec<String>,
}

#[derive(Parser)]
pub struct Build {
    /// Go source files or directories to build
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// Runnable executable output path
    #[arg(short, long)]
    pub output: Option<String>,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    pub timings_json: Option<String>,
    /// Maximum number of compiler jobs
    #[arg(long, value_name = "N", default_value_t = default_job_budget())]
    pub jobs: NonZeroUsize,
}

#[derive(Parser)]
pub struct EmitRust {
    /// Go source files or directories to compile
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// Generated Rust output directory
    #[arg(short, long, required = true, value_name = "DIRECTORY")]
    pub output: String,
    /// Output path for a Source Map v3 file
    #[arg(long, value_name = "PATH")]
    pub sourcemap: Option<String>,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    pub timings_json: Option<String>,
    /// Maximum number of compiler jobs
    #[arg(long, value_name = "N", default_value_t = default_job_budget())]
    pub jobs: NonZeroUsize,
}

#[derive(Parser)]
pub struct Run {
    /// Build in release mode, with optimizations
    #[arg(long)]
    pub release: bool,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    pub timings_json: Option<String>,
    /// Maximum number of compiler jobs
    #[arg(long, value_name = "N", default_value_t = default_job_budget())]
    pub jobs: NonZeroUsize,
    /// Go source files or directories
    #[arg(required = true, num_args = 1..)]
    pub paths: Vec<String>,
    /// Arguments passed to the program after a literal `--`
    #[arg(last = true, allow_hyphen_values = true, value_name = "ARG")]
    pub program_args: Vec<String>,
}

#[derive(Parser)]
pub struct Help {
    /// Print help for the command(s)
    #[arg(value_name = "COMMAND", num_args = 0.., trailing_var_arg = true)]
    pub commands: Vec<String>,
}

#[derive(Parser)]
pub struct Tokens {
    /// The files to lex
    #[arg(required = true)]
    pub files: Vec<String>,
}
