// Clippy lints are configured at workspace level in the root Cargo.toml

mod cache;
mod timings;

use cache::{
    CacheAccessLock, CacheRequest, CacheRequestOptions, CliCacheManifest, FileArtifact,
    InputSnapshot, export_resolver_cache, generated_file_hashes, import_resolver_cache,
    maybe_prune_cli_cache, remove_legacy_incremental,
};
use clap::{CommandFactory, Parser};
use gors::error::{Diagnostic, DiagnosticKind};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use timings::TimingCollector;

const RUST_TOOLCHAIN: &str = "1.96.0";
const RUST_EDITION: &str = "2024";

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
    /// Maximum compiler package tasks to run concurrently
    #[arg(long, value_parser = parse_jobs)]
    jobs: Option<usize>,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    timings_json: Option<String>,
}

#[derive(Parser)]
struct Run {
    /// Build in release mode, with optimizations
    #[arg(long)]
    release: bool,
    /// Maximum compiler package tasks to run concurrently
    #[arg(long, value_parser = parse_jobs)]
    jobs: Option<usize>,
    /// Write machine-readable phase timings to this JSON file
    #[arg(long, value_name = "PATH")]
    timings_json: Option<String>,
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
    let jobs = resolved_jobs(cmd.jobs)?;
    let timings = TimingCollector::new();
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
        jobs,
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
        timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build", jobs)?;
        drop(output_lock);
        drop(cache_access_lock);
        return Ok(());
    }
    timings.cache_event("compiler", false);

    let parse_timer = timings.phase("cli.parse");
    let program = match gors::parser::parse_program(&cmd.path) {
        Ok(result) => result,
        Err(gors::parser::PathParseError::ParserError(err)) => {
            let (file, buffer) = if let Some((f, b)) = get_file_for_error(&cmd.path) {
                (f, b)
            } else {
                (cmd.path.clone(), String::new())
            };
            let diagnostic = Diagnostic::from_parser_error(&err, &file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("error: {}", err);
            std::process::exit(1);
        }
    };
    drop(parse_timer);
    let inputs = InputSnapshot::capture(&program, &source_paths)?;
    let uses_stdlib = !program.stdlib_imports.is_empty();
    if uses_stdlib {
        timings.cache_event("resolver", import_resolver_cache(&cache_base));
    }

    let primary_file = program
        .main_package
        .files
        .first()
        .map(|(f, _)| f.clone())
        .unwrap_or_else(|| cmd.path.clone());

    let compile_timer = timings.phase("cli.compile");
    let compiled = match compile_program(program, jobs, sourcemap_path.is_some()) {
        Ok(compiled) => compiled,
        Err(err) => {
            let diagnostic = Diagnostic::new(
                &primary_file,
                0,
                0,
                err.to_string(),
                DiagnosticKind::Compiler,
            );
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };
    drop(compile_timer);
    if uses_stdlib {
        let _ = export_resolver_cache(&cache_base);
    }

    let print_timer = timings.phase("cli.print");
    let output = gors::printer::generate_multi(compiled)?;
    drop(print_timer);
    let generated_files = generated_file_hashes(&output);
    let write_timer = timings.phase("cli.file_writes");
    let stats = write_generated_output_locked(&output, &output_dir)?;
    let sourcemap = sourcemap_path
        .as_deref()
        .map(|path| write_source_map(&output, path))
        .transpose()?;
    drop(write_timer);

    let completed_request = CacheRequest::new(CacheRequestOptions {
        command: "build",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&output_dir),
        sourcemap: sourcemap_path.as_deref(),
        jobs,
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

    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "build", jobs)?;
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
    let prev_manifest = gors::compiler::manifest::BuildManifest::load(output_dir);
    let mut new_manifest = gors::compiler::manifest::BuildManifest::new();
    let mut stats = FileWriteStats {
        written: 0,
        skipped: 0,
        removed: 0,
    };
    let mut pending_writes = Vec::new();

    for (filename, source) in &output.files {
        let file_path = output_dir.join(filename);
        let current_hash = sha2_hash(source);
        let unchanged = prev_manifest
            .as_ref()
            .is_some_and(|manifest| !manifest.needs_recompile(filename, &current_hash));

        if unchanged && file_path.exists() {
            stats.skipped += 1;
        } else {
            let temp = prepare_atomic_write(&file_path, source)?;
            pending_writes.push((file_path, temp));
            stats.written += 1;
        }

        new_manifest.modules.insert(
            filename.clone(),
            gors::compiler::manifest::ModuleEntry {
                content_hash: current_hash,
                output_file: filename.clone(),
            },
        );
    }

    // Publish leaf modules before the coordinator files that reference them.
    // Each rename is atomic, while the directory lock prevents concurrent gors
    // builds from interleaving two generated programs in the same directory.
    pending_writes.sort_by_key(|(path, _)| pending_write_priority(path));
    for (file_path, temp) in pending_writes {
        temp.persist(file_path).map_err(|error| error.error)?;
    }

    if let Some(prev_manifest) = &prev_manifest {
        for (filename, entry) in &prev_manifest.modules {
            if output.files.contains_key(filename) {
                continue;
            }
            let file_path = output_dir.join(&entry.output_file);
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

fn parse_jobs(value: &str) -> Result<usize, String> {
    let jobs = value
        .parse::<usize>()
        .map_err(|_| format!("'{value}' is not a positive integer"))?;
    if jobs == 0 {
        return Err("job count must be at least 1".to_string());
    }
    Ok(jobs)
}

fn resolved_jobs(cli_jobs: Option<usize>) -> Result<usize, Box<dyn std::error::Error>> {
    if let Some(jobs) = cli_jobs {
        return Ok(jobs);
    }
    if let Ok(value) = std::env::var("GORS_JOBS") {
        return parse_jobs(&value).map_err(Into::into);
    }
    Ok(std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get))
}

fn compile_program(
    program: gors::parser::ParsedProgram,
    jobs: usize,
    source_maps: bool,
) -> Result<gors::compiler::CompiledProgram, gors::compiler::CompilerError> {
    let options = gors::compiler::CompileOptions::with_jobs(jobs);
    if source_maps {
        gors::compiler::compile_program_multi_with_source_maps_and_options(program, options)
    } else {
        gors::compiler::compile_program_multi_with_options(program, options)
    }
}

fn write_source_map(
    output: &gors::printer::GeneratedOutput,
    path: &Path,
) -> Result<FileArtifact, Box<dyn std::error::Error>> {
    let main_source = output
        .files
        .get("main.rs")
        .ok_or("generated program has no main.rs for source-map output")?;
    let source_map = gors::compiler::build_source_map(main_source);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    source_map.to_writer(temp.as_file_mut())?;
    temp.as_file_mut().sync_all()?;
    gors::compiler::clear_source_map_tracker();
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
    let jobs = resolved_jobs(cmd.jobs)?;
    let timings = TimingCollector::new();
    let cache_base = gors_cache_base()?;
    let cache_dir = run_cache_dir(&source_paths, cmd.release)?;
    maybe_prune_cli_cache(&cache_base, Some(&cache_dir))?;
    let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base)?;
    let cache_lock = OutputDirectoryLock::acquire(&cache_dir)?;
    remove_legacy_incremental(&cache_dir)?;
    let request = CacheRequest::new(CacheRequestOptions {
        command: "run",
        source_paths: &source_paths,
        release: cmd.release,
        output: Some(&cache_dir),
        sourcemap: None,
        jobs,
    })?;
    let mut cache_manifest = {
        let _cache_timer = timings.phase("cli.cache_lookup");
        CliCacheManifest::load_if_generated_valid(&cache_dir, &request)
    };

    if cache_manifest.is_some() {
        timings.cache_event("compiler", true);
    } else {
        timings.cache_event("compiler", false);
        let parse_timer = timings.phase("cli.parse");
        let program = match gors::parser::parse_program_files(&source_paths) {
            Ok(result) => result,
            Err(gors::parser::PathParseError::ParserError(err)) => {
                let source_path = source_paths.first().cloned().unwrap_or_default();
                let (file, buffer) = if let Some((f, b)) = get_file_for_error(&source_path) {
                    (f, b)
                } else {
                    (source_path, String::new())
                };
                let diagnostic = Diagnostic::from_parser_error(&err, &file, &buffer);
                print_error(&diagnostic);
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("error: {}", err);
                std::process::exit(1);
            }
        };
        drop(parse_timer);
        let inputs = InputSnapshot::capture(&program, &source_paths)?;
        let uses_stdlib = !program.stdlib_imports.is_empty();
        if uses_stdlib {
            timings.cache_event("resolver", import_resolver_cache(&cache_base));
        }

        let primary_file = program
            .main_package
            .files
            .first()
            .map(|(f, _)| f.clone())
            .unwrap_or_else(|| source_paths.first().cloned().unwrap_or_default());

        let compile_timer = timings.phase("cli.compile");
        let compiled = match compile_program(program, jobs, false) {
            Ok(compiled) => compiled,
            Err(err) => {
                let diagnostic = Diagnostic::new(
                    &primary_file,
                    0,
                    0,
                    err.to_string(),
                    DiagnosticKind::Compiler,
                );
                print_error(&diagnostic);
                std::process::exit(1);
            }
        };
        drop(compile_timer);
        if uses_stdlib {
            let _ = export_resolver_cache(&cache_base);
        }

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
            jobs,
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
    timings.write_json(cmd.timings_json.as_deref().map(Path::new), "run", jobs)?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Helper to get file path and contents for error reporting.
/// If path is a directory, returns the first .go file in it.
fn get_file_for_error(path: &str) -> Option<(String, String)> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.is_file() {
        let buffer = std::fs::read_to_string(path).ok()?;
        Some((path.to_string(), buffer))
    } else if metadata.is_dir() {
        let entries = std::fs::read_dir(path).ok()?;
        for entry in entries.flatten() {
            let file_path = entry.path();
            if let Some(name) = file_path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".go") && !name.ends_with("_test.go") && !name.starts_with('.') {
                    let buffer = std::fs::read_to_string(&file_path).ok()?;
                    return Some((file_path.to_string_lossy().into_owned(), buffer));
                }
            }
        }
        None
    } else {
        None
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
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    const LOCK_CHILD_DIRECTORY_ENV: &str = "GORS_TEST_LOCK_CHILD_DIRECTORY";

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn split_run_args_keeps_single_file_as_source() {
        let (sources, program_args) = split_run_args(&args(&["main.go", "--", "arg"]));
        assert_eq!(sources, args(&["main.go"]));
        assert_eq!(program_args, args(&["--", "arg"]));
    }

    #[test]
    fn split_run_args_groups_leading_go_files() {
        let (sources, program_args) = split_run_args(&args(&["main.go", "helpers.go", "--flag"]));
        assert_eq!(sources, args(&["main.go", "helpers.go"]));
        assert_eq!(program_args, args(&["--flag"]));
    }

    #[test]
    fn split_run_args_treats_directory_as_single_source() {
        let (sources, program_args) = split_run_args(&args(&[".", "--flag", "value"]));
        assert_eq!(sources, args(&["."]));
        assert_eq!(program_args, args(&["--flag", "value"]));
    }

    #[test]
    fn split_run_args_treats_package_path_as_single_source() {
        let (sources, program_args) = split_run_args(&args(&["./cmd/myapp", "arg"]));
        assert_eq!(sources, args(&["./cmd/myapp"]));
        assert_eq!(program_args, args(&["arg"]));
    }

    #[test]
    fn build_accepts_jobs_and_timing_report_options() {
        let opts = Opts::try_parse_from([
            "gors",
            "build",
            "--jobs",
            "4",
            "--timings-json",
            "timings.json",
            "main.go",
        ])
        .unwrap();
        let SubCommand::Build(build) = opts.subcmd else {
            panic!("expected build command");
        };
        assert_eq!(build.jobs, Some(4));
        assert_eq!(build.timings_json.as_deref(), Some("timings.json"));
        assert_eq!(build.path, "main.go");
    }

    #[test]
    fn run_accepts_jobs_before_trailing_program_arguments() {
        let opts = Opts::try_parse_from([
            "gors",
            "run",
            "--jobs",
            "2",
            "--timings-json",
            "timings.json",
            "main.go",
            "--program-flag",
        ])
        .unwrap();
        let SubCommand::Run(run) = opts.subcmd else {
            panic!("expected run command");
        };
        assert_eq!(run.jobs, Some(2));
        assert_eq!(run.timings_json.as_deref(), Some("timings.json"));
        assert_eq!(run.args, args(&["main.go", "--program-flag"]));
    }

    #[test]
    fn jobs_rejects_zero() {
        assert!(Opts::try_parse_from(["gors", "build", "--jobs", "0", "main.go"]).is_err());
    }

    #[test]
    fn rustc_arguments_do_not_create_per_invocation_incremental_state() {
        let flags = Vec::from(RustcArgs {
            src: "main.rs",
            out: Some("main"),
            emit: None,
            release: false,
        });
        assert!(
            flags.iter().all(|flag| !flag.contains("incremental")),
            "{flags:?}"
        );
    }

    #[test]
    fn write_generated_output_removes_files_missing_from_new_manifest() {
        let tmp = tempfile::tempdir().unwrap();

        let mut first_files = BTreeMap::new();
        first_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
        first_files.insert("stale.rs".to_string(), "fn stale() {}\n".to_string());
        let first = gors::printer::GeneratedOutput { files: first_files };
        let first_stats = write_generated_output(&first, tmp.path()).unwrap();
        assert_eq!(first_stats.written, 2);
        assert!(tmp.path().join("stale.rs").exists());

        let mut second_files = BTreeMap::new();
        second_files.insert("main.rs".to_string(), "fn main() {}\n".to_string());
        let second = gors::printer::GeneratedOutput {
            files: second_files,
        };
        let second_stats = write_generated_output(&second, tmp.path()).unwrap();

        assert_eq!(second_stats.skipped, 1);
        assert_eq!(second_stats.removed, 1);
        assert!(!tmp.path().join("stale.rs").exists());
    }

    #[test]
    fn concurrent_output_publications_publish_one_consistent_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let output_dir = tmp.path().to_path_buf();
        let source_map_path = output_dir.join("program.map");
        let executable_path = output_dir.join("main");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let active_publishers = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let handles: Vec<_> = [("alpha", "fn alpha() {}\n"), ("beta", "fn beta() {}\n")]
            .into_iter()
            .map(|(module, body)| {
                let output_dir = output_dir.clone();
                let source_map_path = source_map_path.clone();
                let executable_path = executable_path.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                let active_publishers = std::sync::Arc::clone(&active_publishers);
                std::thread::spawn(move || {
                    let source_path = output_dir.join(format!("{module}.go"));
                    std::fs::write(
                        &source_path,
                        format!("package main\n\nfunc main() {{ println(\"{module}\") }}\n"),
                    )
                    .unwrap();
                    let source_paths = vec![source_path.to_string_lossy().into_owned()];
                    let program = gors::parser::parse_program(
                        source_paths.first().expect("single source path"),
                    )
                    .unwrap();
                    let inputs = InputSnapshot::capture(&program, &source_paths).unwrap();
                    let request = CacheRequest::new(CacheRequestOptions {
                        command: "run",
                        source_paths: &source_paths,
                        release: false,
                        output: Some(&output_dir),
                        sourcemap: Some(&source_map_path),
                        jobs: 1,
                    })
                    .unwrap();
                    let mut files = BTreeMap::new();
                    files.insert(
                        "main.rs".to_string(),
                        format!("mod {module};\nfn main() {{ {module}(); }}\n"),
                    );
                    files.insert(format!("{module}.rs"), body.to_string());
                    let output = gors::printer::GeneratedOutput { files };
                    let generated_files = generated_file_hashes(&output);
                    barrier.wait();

                    let _output_lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
                    assert_eq!(
                        active_publishers.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                        0,
                        "publication transactions overlapped"
                    );
                    std::thread::sleep(Duration::from_millis(25));
                    write_generated_output_locked(&output, &output_dir).unwrap();
                    prepare_atomic_write(&source_map_path, &format!("{module}-map\n"))
                        .unwrap()
                        .persist(&source_map_path)
                        .unwrap();
                    let source_map = FileArtifact::capture(&source_map_path).unwrap();
                    let mut manifest =
                        CliCacheManifest::new(&request, inputs, generated_files, Some(source_map));
                    manifest.save(&output_dir).unwrap();
                    prepare_atomic_write(&executable_path, &format!("{module}-executable\n"))
                        .unwrap()
                        .persist(&executable_path)
                        .unwrap();
                    manifest.set_executable(&executable_path).unwrap();
                    manifest.save(&output_dir).unwrap();
                    assert_eq!(
                        active_publishers.fetch_sub(1, std::sync::atomic::Ordering::SeqCst),
                        1
                    );
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let manifest =
            gors::compiler::manifest::BuildManifest::load(&output_dir).expect("manifest");
        let main = std::fs::read_to_string(output_dir.join("main.rs")).unwrap();
        let published = if manifest.modules.contains_key("alpha.rs") {
            "alpha"
        } else {
            "beta"
        };
        let stale = if published == "alpha" {
            "beta"
        } else {
            "alpha"
        };

        assert!(main.contains(published));
        assert!(output_dir.join(format!("{published}.rs")).exists());
        assert!(!manifest.modules.contains_key(&format!("{stale}.rs")));
        assert!(!output_dir.join(format!("{stale}.rs")).exists());
        assert_eq!(
            std::fs::read_to_string(&source_map_path).unwrap(),
            format!("{published}-map\n")
        );
        assert_eq!(
            std::fs::read_to_string(&executable_path).unwrap(),
            format!("{published}-executable\n")
        );

        let source_paths = vec![
            output_dir
                .join(format!("{published}.go"))
                .to_string_lossy()
                .into_owned(),
        ];
        let request = CacheRequest::new(CacheRequestOptions {
            command: "run",
            source_paths: &source_paths,
            release: false,
            output: Some(&output_dir),
            sourcemap: Some(&source_map_path),
            jobs: 1,
        })
        .unwrap();
        let cli_manifest = CliCacheManifest::load_if_generated_valid(&output_dir, &request)
            .expect("published CLI cache manifest");
        assert!(cli_manifest.executable_is_valid(&executable_path));
    }

    #[test]
    fn spawned_program_does_not_retain_publication_locks_while_running() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_base = tmp.path().join("cache");
        let output_dir = cache_base.join("run").join("entry");
        let cache_access_lock = CacheAccessLock::acquire_shared(&cache_base).unwrap();
        let output_lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "tests::output_lock_child_waits_for_release",
                "--exact",
                "--nocapture",
            ])
            .env(LOCK_CHILD_DIRECTORY_ENV, tmp.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = output_lock.spawn_and_release(&mut command).unwrap();
        drop(cache_access_lock);
        let started = tmp.path().join("child-started");
        let release = tmp.path().join("child-release");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !started.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.is_file(), "child process did not start");
        assert!(
            child.try_wait().unwrap().is_none(),
            "child process exited before the lock check"
        );

        let (acquired_tx, acquired_rx) = mpsc::channel();
        let contender = std::thread::spawn(move || {
            let _lock = OutputDirectoryLock::acquire(&output_dir).unwrap();
            acquired_tx.send(()).unwrap();
        });
        let acquired_while_running = acquired_rx.recv_timeout(Duration::from_secs(2)).is_ok();

        let (pruned_tx, pruned_rx) = mpsc::channel();
        let pruner = std::thread::spawn(move || {
            maybe_prune_cli_cache(&cache_base, None).unwrap();
            pruned_tx.send(()).unwrap();
        });
        let pruned_while_running = pruned_rx.recv_timeout(Duration::from_secs(2)).is_ok();

        std::fs::write(release, b"release").unwrap();
        let status = child.wait().unwrap();
        contender.join().unwrap();
        pruner.join().unwrap();

        assert!(
            acquired_while_running,
            "output lock remained held for the child process lifetime"
        );
        assert!(
            pruned_while_running,
            "cache-wide shared lock remained held for the child process lifetime"
        );
        assert!(status.success());
    }

    #[test]
    fn output_lock_child_waits_for_release() {
        let Some(directory) = std::env::var_os(LOCK_CHILD_DIRECTORY_ENV) else {
            return;
        };
        let directory = PathBuf::from(directory);
        std::fs::write(directory.join("child-started"), b"started").unwrap();
        let release = directory.join("child-release");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(release.is_file(), "parent did not release child process");
    }
}
