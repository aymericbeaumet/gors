// Clippy lints are configured at workspace level in the root Cargo.toml

mod cache;
mod timings;

use cache::{
    CacheAccessLock, CacheRequest, CacheRequestOptions, CliCacheManifest, FileArtifact,
    GeneratedOutputManifest, InputSnapshot, generated_file_hashes, maybe_prune_cli_cache,
};
use clap::{CommandFactory, Parser};
use gors::compiler::input::{InputError, WorkspaceKey};
use gors::error::{Diagnostic, DiagnosticKind};
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use timings::TimingCollector;

const RUST_TOOLCHAIN: &str = "1.96.0";
const RUST_EDITION: &str = "2024";
const CLI_WORKSPACE_KEY: &str = "gors-cli";

fn cli_workspace() -> Result<WorkspaceKey, InputError> {
    WorkspaceKey::ad_hoc(CLI_WORKSPACE_KEY)
}

fn default_job_budget() -> NonZeroUsize {
    std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Help(cmd) => help(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
        SubCommand::Version => version(),
    }
}

/// Print a formatted error with source context
fn print_error(diagnostic: &Diagnostic) {
    // Check if stdout supports colors
    let use_colors = atty::is(atty::Stream::Stderr);
    eprint!("{}", diagnostic.format_terminal(use_colors));
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
struct Opts {
    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
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
struct Ast {
    /// The files to parse
    #[arg(required = true)]
    files: Vec<String>,
}

#[derive(Parser)]
struct Build {
    /// The Go source file or directory to build
    path: String,
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Output path for source map (.map file in standard v3 format)
    #[arg(long)]
    sourcemap: Option<String>,
    /// Output file path
    #[arg(short, long)]
    output: Option<String>,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    timings_json: Option<String>,
    /// Maximum number of compiler jobs
    #[arg(long, value_name = "N", default_value_t = default_job_budget())]
    jobs: NonZeroUsize,
}

#[derive(Parser)]
struct Run {
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    timings_json: Option<String>,
    /// Maximum number of compiler jobs
    #[arg(long, value_name = "N", default_value_t = default_job_budget())]
    jobs: NonZeroUsize,
    /// Go source file(s), directory, or package path, followed by optional program arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    args: Vec<String>,
}

#[derive(Parser)]
struct Help {
    /// Print help for the command(s)
    #[arg(value_name = "COMMAND", num_args = 0.., trailing_var_arg = true)]
    commands: Vec<String>,
}

#[derive(Parser)]
struct Tokens {
    /// The files to lex
    #[arg(required = true)]
    files: Vec<String>,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    match cmd.files.as_slice() {
        [file] => ast_single(file, &mut w)?,
        files => {
            for file in files {
                write_file_result(&mut w, file, ast_output(file))?;
            }
        }
    }
    w.flush()?;

    Ok(())
}

fn ast_single(file: &str, w: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    let ast = match gors::parser::parse_file(file, &buffer) {
        Ok(ast) => ast,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };
    gors::ast::fprint(w, ast)?;
    Ok(())
}

fn ast_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let ast = gors::parser::parse_file(file, &buffer).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    gors::ast::fprint(&mut output, ast).map_err(|e| e.to_string())?;
    Ok(output)
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let timings = TimingCollector::new(cmd.jobs);
    let compiler_host = gors::compiler::CompilerHost::new(cmd.jobs)?;
    let cache_base = gors_cache_base()?;
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
    maybe_prune_cli_cache(&cache_base, Some(&output_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base)?;
    let output_lock = OutputDirectoryLock::acquire(&output_dir)?;

    let cached_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&output_dir, &request)
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

    let source_load_timer = timings.phase("cli.source_load");
    let loaded = gors::workspace::load_program(cli_workspace()?, &cmd.path)?;
    drop(source_load_timer);
    let inputs = InputSnapshot::capture(&loaded)?;
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

fn build_cache_dir(source_path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    hasher.update(b"\0");
    hasher.update(source_path.as_bytes());
    if let Ok(canonical) = std::fs::canonicalize(source_path) {
        hasher.update(b"\0");
        hasher.update(canonical.to_string_lossy().as_bytes());
    }

    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("build").join(key))
}

struct FileWriteStats {
    written: usize,
    skipped: usize,
    removed: usize,
}

struct OutputDirectoryLock {
    _file: std::fs::File,
}

impl OutputDirectoryLock {
    fn acquire(output_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(output_dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(output_dir.join(".gors-build.lock"))?;
        file.lock()?;
        Ok(Self { _file: file })
    }

    fn spawn_and_release(
        self,
        command: &mut Command,
    ) -> Result<std::process::Child, std::io::Error> {
        let child = command.spawn()?;
        drop(self);
        Ok(child)
    }
}

fn pending_write_priority(path: &Path) -> u8 {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs") => 1,
        Some("main.rs") => 2,
        _ => 0,
    }
}

fn prepare_atomic_write(
    path: &Path,
    source: &str,
) -> Result<tempfile::NamedTempFile, Box<dyn std::error::Error>> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(source.as_bytes())?;
    temp.as_file_mut().sync_all()?;
    Ok(temp)
}

#[cfg(test)]
fn write_generated_output(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<FileWriteStats, Box<dyn std::error::Error>> {
    let _output_lock = OutputDirectoryLock::acquire(output_dir)?;
    write_generated_output_locked(output, output_dir)
}

fn write_generated_output_locked(
    output: &gors::printer::GeneratedOutput,
    output_dir: &Path,
) -> Result<FileWriteStats, Box<dyn std::error::Error>> {
    let previous_manifest = GeneratedOutputManifest::load(output_dir);
    let mut new_manifest = GeneratedOutputManifest::new();
    let mut stats = FileWriteStats {
        written: 0,
        skipped: 0,
        removed: 0,
    };
    let mut pending_writes = Vec::new();

    for (filename, source) in &output.files {
        let file_path = output_dir.join(filename);
        let current_hash = sha2_hash(source);
        let unchanged = previous_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.matches(filename, &current_hash));

        if unchanged && file_path.exists() {
            stats.skipped += 1;
        } else {
            let temp = prepare_atomic_write(&file_path, source)?;
            pending_writes.push((file_path, temp));
            stats.written += 1;
        }

        new_manifest.record(filename.clone(), current_hash);
    }

    // Publish leaf modules before the coordinator files that reference them.
    // Each rename is atomic, while the directory lock prevents concurrent gors
    // builds from interleaving two generated programs in the same directory.
    pending_writes.sort_by_key(|(path, _)| pending_write_priority(path));
    for (file_path, temp) in pending_writes {
        temp.persist(file_path).map_err(|error| error.error)?;
    }

    if let Some(previous_manifest) = &previous_manifest {
        for (filename, output_file) in previous_manifest.files() {
            if output.files.contains_key(filename) {
                continue;
            }
            let file_path = output_dir.join(output_file);
            if file_path.is_file() {
                std::fs::remove_file(&file_path)?;
                stats.removed += 1;
            }
        }
    }

    new_manifest.save(output_dir)?;
    Ok(stats)
}

fn sha2_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn run_cache_dir(
    source_paths: &[String],
    release: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(if release {
        b"release".as_slice()
    } else {
        b"debug".as_slice()
    });
    hasher.update(b"\0");
    if let Ok(cwd) = std::env::current_dir() {
        hasher.update(cwd.to_string_lossy().as_bytes());
    }
    for path in source_paths {
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
        if let Ok(canonical) = std::fs::canonicalize(path) {
            hasher.update(b"\0");
            hasher.update(canonical.to_string_lossy().as_bytes());
        }
    }
    let digest = hasher.finalize();
    let key: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(gors_cache_base()?.join("run").join(key))
}

fn gors_cache_base() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("gors"));
    }
    if let Some(path) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(path).join(".cache").join("gors"));
    }
    Ok(std::env::temp_dir().join("gors-cache"))
}

fn compile_program(
    program: gors::compiler::input::ProgramInput,
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

fn write_source_map(
    output: &gors::printer::GeneratedOutput,
    plan: &gors::compiler::SourceMapPlan,
    path: &Path,
) -> Result<FileArtifact, Box<dyn std::error::Error>> {
    let main_source = output
        .files
        .get("main.rs")
        .ok_or("generated program has no main.rs for source-map output")?;
    let source_map = plan.build(main_source);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    source_map.to_writer(temp.as_file_mut())?;
    temp.as_file_mut().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    FileArtifact::capture(path)
}

fn compile_generated_binary(
    cache_dir: &Path,
    bin_path: &Path,
    release: bool,
    timings: &TimingCollector,
) -> Result<(), Box<dyn std::error::Error>> {
    let src_path = cache_dir.join("main.rs");
    let pending_path = cache_dir.join(format!(".main-{}.pending", std::process::id()));
    if pending_path.exists() {
        std::fs::remove_file(&pending_path)?;
    }

    let src_str = src_path.to_string_lossy();
    let pending_str = pending_path.to_string_lossy();
    let rustc_args = RustcArgs {
        src: &src_str,
        out: Some(&pending_str),
        emit: None,
        release,
    };

    let rustc_timer = timings.phase("cli.rustc");
    let rustc_status = Command::new("rustup")
        .args(["run", RUST_TOOLCHAIN, "rustc"])
        .args(Vec::from(rustc_args))
        .status()?;
    drop(rustc_timer);
    if !rustc_status.success() {
        if pending_path.exists() {
            std::fs::remove_file(&pending_path)?;
        }
        std::process::exit(rustc_status.code().unwrap_or(1));
    }
    std::fs::rename(pending_path, bin_path)?;
    Ok(())
}

/// Split CLI arguments into source paths and program arguments.
///
/// If the first argument ends with `.go`, all leading `.go` arguments are source
/// files. Otherwise, the first argument is a directory/package path. Everything
/// after the source paths is passed through to the compiled program.
fn split_run_args(args: &[String]) -> (Vec<String>, Vec<String>) {
    if args.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if args.first().is_some_and(|a| a.ends_with(".go")) {
        let split = args
            .iter()
            .position(|a| !a.ends_with(".go"))
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

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let (source_paths, program_args) = split_run_args(&cmd.args);
    let timings = TimingCollector::new(cmd.jobs);
    let compiler_host = gors::compiler::CompilerHost::new(cmd.jobs)?;
    let cache_base = gors_cache_base()?;
    let cache_dir = run_cache_dir(&source_paths, cmd.release)?;
    maybe_prune_cli_cache(&cache_base, Some(&cache_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base)?;
    let cache_lock = OutputDirectoryLock::acquire(&cache_dir)?;
    let request = CacheRequest::new(CacheRequestOptions {
        command: "run",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&cache_dir),
        sourcemap: None,
    })?;
    let mut cache_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&cache_dir, &request)
    };

    if cache_manifest.is_some() {
        timings.cache_event("compiler", true);
    } else {
        timings.cache_event("compiler", false);
        let source_load_timer = timings.phase("cli.source_load");
        let loaded = gors::workspace::load_program_files(cli_workspace()?, &source_paths)?;
        drop(source_load_timer);
        let inputs = InputSnapshot::capture(&loaded)?;
        let primary_file = loaded.primary_diagnostic_path().to_string();

        let compile_timer = timings.phase("cli.compile");
        let compilation = compile_program(loaded.into_input(), false, &compiler_host);
        timings.scheduler_telemetry(compiler_host.telemetry());
        drop(compiler_host);
        let compiled = match compilation {
            Ok((compiled, None)) => compiled,
            Ok((_, Some(_))) => {
                return Err("unexpected source-map plan for run compilation".into());
            }
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
        write_generated_output_locked(&output, &cache_dir)?;
        drop(write_timer);

        let completed_request = CacheRequest::new(CacheRequestOptions {
            command: "run",
            source_paths: &source_paths,
            release: cmd.release,
            output: Some(&cache_dir),
            sourcemap: None,
        })?;
        if request == completed_request {
            let manifest = CliCacheManifest::new(&request, inputs, generated_files, None);
            manifest.save(&cache_dir)?;
            cache_manifest = Some(manifest);
        }
    }

    let Some(mut cache_manifest) = cache_manifest else {
        return Err("source inputs changed while compiling; rerun the command".into());
    };
    let bin_path = cache_dir.join("main");
    if cache_manifest.executable_is_valid(&bin_path) {
        timings.cache_event("rustc", true);
    } else {
        timings.cache_event("rustc", false);
        compile_generated_binary(&cache_dir, &bin_path, cmd.release, &timings)?;
        cache_manifest.set_executable(&bin_path)?;
        cache_manifest.save(&cache_dir)?;
    }

    let execute_timer = timings.phase("cli.execute");
    let mut command = Command::new(&bin_path);
    command.args(&program_args);
    let mut child = cache_lock.spawn_and_release(&mut command)?;
    drop(cache_access_lock);
    let status = child.wait()?;
    drop(execute_timer);
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "run")?;
    std::process::exit(status.code().unwrap_or(1));
}

fn print_compiler_error(error: &gors::compiler::CompilerError, fallback_file: &str) {
    for diagnostic in error.diagnostics() {
        let file = if diagnostic.file.is_empty() {
            fallback_file
        } else {
            &diagnostic.file
        };
        print_error(&Diagnostic::new(
            file,
            diagnostic.line,
            diagnostic.column,
            format!("{}: {}", diagnostic.code, diagnostic.message),
            DiagnosticKind::Compiler,
        ));
    }
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    match cmd.files.as_slice() {
        [file] => tokens_single(file, &mut w)?,
        files => {
            for file in files {
                write_file_result(&mut w, file, tokens_output(file))?;
            }
        }
    }
    w.flush()?;

    Ok(())
}

fn tokens_single(file: &str, w: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    for step in gors::scanner::Scanner::new(file, &buffer) {
        match step {
            Ok(s) => {
                serde_json::to_writer(&mut *w, &s)?;
                w.write_all(b"\n")?;
            }
            Err(err) => {
                let diagnostic = Diagnostic::from_scanner_error(&err, file, &buffer);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn tokens_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    for step in gors::scanner::Scanner::new(file, &buffer) {
        let token = step.map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut output, &token).map_err(|e| e.to_string())?;
        output.write_all(b"\n").map_err(|e| e.to_string())?;
    }
    Ok(output)
}

fn write_file_result(
    w: &mut impl Write,
    path: &str,
    result: Result<Vec<u8>, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match result {
        Ok(stdout) => serde_json::to_writer(
            &mut *w,
            &serde_json::json!({
                "path": path,
                "ok": true,
                "stdout": String::from_utf8_lossy(&stdout),
            }),
        )?,
        Err(stderr) => serde_json::to_writer(
            &mut *w,
            &serde_json::json!({
                "path": path,
                "ok": false,
                "stderr": stderr,
            }),
        )?,
    }
    w.write_all(b"\n")?;
    Ok(())
}

fn help(cmd: Help) -> Result<(), Box<dyn std::error::Error>> {
    let mut root = Opts::command();
    let mut current = &mut root;
    for command in &cmd.commands {
        let Some(next) = current.find_subcommand_mut(command) else {
            eprintln!("error: unknown command '{command}'");
            std::process::exit(2);
        };
        current = next;
    }
    if !cmd.commands.is_empty() {
        current.set_bin_name(format!("gors {}", cmd.commands.join(" ")));
    }
    current.print_help()?;
    println!();
    Ok(())
}

fn version() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "gors version gors{} {} {}/{}",
        env!("CARGO_PKG_VERSION"),
        gors::STDLIB_VERSION,
        go_target_os(),
        go_target_arch()
    );
    Ok(())
}

fn go_target_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    }
}

fn go_target_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86" => "386",
        "x86_64" => "amd64",
        arch => arch,
    }
}

struct RustcArgs<'a> {
    src: &'a str,
    out: Option<&'a str>,
    emit: Option<&'a str>,
    release: bool,
}

impl<'a> From<RustcArgs<'a>> for Vec<String> {
    fn from(args: RustcArgs<'a>) -> Self {
        let mut flags = vec![
            args.src.to_string(),
            format!("--edition={RUST_EDITION}"),
            "-D".to_string(),
            "unused_imports".to_string(),
            "-D".to_string(),
            "unused_macros".to_string(),
            "-C".to_string(),
            "overflow-checks=off".to_string(),
        ];

        if let Some(emit) = args.emit {
            flags.extend(["--emit".to_string(), emit.to_string()]);
        }

        if let Some(out) = args.out {
            flags.extend(["-o".to_string(), out.to_string()]);
        }

        if args.release {
            flags.extend([
                "-Ccodegen-units=1".to_string(),
                "-Clto=fat".to_string(),
                "-Copt-level=3".to_string(),
                "-Ctarget-cpu=native".to_string(),
            ]);
        }

        flags
    }
}

impl<'a> IntoIterator for RustcArgs<'a> {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        Vec::from(self).into_iter()
    }
}

#[cfg(test)]
mod tests;
