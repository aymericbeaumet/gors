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
    about = "gors is a go toolbelt written in rust; providing a parser and rust transpiler",
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
    /// Transpile Go source to Rust files, writing to cache or --output
    #[command(display_order = 0)]
    Build(Build),
    /// Print this message or the help of the given command(s)
    #[command(display_order = 0)]
    Help(Help),
    /// Transpile, compile, and run Go source path(s)
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
    /// The Go source file or directory to build
    pub path: String,
    /// Output path for source map (.map file in standard v3 format)
    #[arg(long)]
    pub sourcemap: Option<String>,
    /// Output file path
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
    /// Go source file(s), directory, or package path, followed by optional program arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub args: Vec<String>,
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
