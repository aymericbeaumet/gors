use clap::Parser;
use std::io::Write;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
    }
}

#[derive(Parser)]
#[clap(
    version = "1.0",
    name = "gors",
    author = "Aymeric Beaumet <hi@aymericbeaumet.com>",
    about = "gors is a go toolbelt written in rust; providing a parser and rust transpiler"
)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(about = "Parse the named Go file and print the AST")]
    Ast(Ast),
    #[clap(about = "Compile the named Go file")]
    Build(Build),
    #[clap(about = "Compile and run the named Go file")]
    Run(Run),
    #[clap(about = "Scan the named Go file and print the tokens")]
    Tokens(Tokens),
}

#[derive(Parser)]
struct Ast {
    #[clap(name = "file", about = "The file to parse")]
    file: String,
}

#[derive(Parser)]
struct Build {
    #[clap(name = "file", about = "The file to build")]
    file: String,
    #[clap(
        long,
        name = "release",
        about = "Build in release mode, with optimizations"
    )]
    release: bool,
    #[clap(
        long,
        name = "emit",
        about = "Type of output for the compiler to emit:\nrust|asm|llvm-bc|llvm-ir|obj|metadata|link|dep-info|mir"
    )]
    emit: Option<String>,
}

#[derive(Parser)]
struct Run {
    #[clap(name = "file", about = "The file to run")]
    file: String,
    #[clap(
        long,
        name = "release",
        about = "Build in release mode, with optimizations"
    )]
    release: bool,
}

#[derive(Parser)]
struct Tokens {
    #[clap(name = "file", about = "The file to lex")]
    file: String,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let buffer = std::fs::read_to_string(&cmd.file)?;
    let ast = gors::parser::parse_file(&cmd.file, &buffer)?;
    gors::ast::fprint(&mut w, ast)?;
    w.flush()?;

    Ok(())
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(&cmd.file)?;
    let ast = gors::parser::parse_file(&cmd.file, &buffer)?;
    let compiled = gors::compiler::compile(ast)?;

    // shortcut when rust code is to be emitted
    if matches!(cmd.emit.as_deref(), Some("rust")) {
        let mut w = std::fs::File::create("main.rs")?;
        gors::codegen::fprint(&mut w, compiled, true)?;
        return Ok(());
    }

    let tmp_dir = tempdir::TempDir::new("gors")?;
    let source_file = tmp_dir.path().join("main.rs");
    let mut w = std::fs::File::create(&source_file)?;
    gors::codegen::fprint(&mut w, compiled, false)?;
    w.sync_all()?;

    let rustc = Command::new("rustc")
        .args(RustcArgs {
            src: source_file.to_str().unwrap(),
            out: None,
            emit: cmd.emit.as_deref(),
            release: cmd.release,
        })
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    Ok(())
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = tempdir::TempDir::new("gors")?;
    let out_rust = tmp_dir.path().join("main.rs");
    let out_bin = tmp_dir.path().join("main");
    let mut w = std::fs::File::create(&out_rust)?;

    let buffer = std::fs::read_to_string(&cmd.file)?;
    let ast = gors::parser::parse_file(&cmd.file, &buffer)?;
    let compiled = gors::compiler::compile(ast)?;
    gors::codegen::fprint(&mut w, compiled, false)?;
    w.sync_all()?;

    let rustc = Command::new("rustc")
        .args(RustcArgs {
            src: out_rust.to_str().unwrap(),
            out: Some(out_bin.to_str().unwrap()),
            emit: None,
            release: cmd.release,
        })
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    let mut cmd = Command::new(&out_bin).spawn()?;
    cmd.wait()?;

    Ok(())
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let buffer = std::fs::read_to_string(&cmd.file)?;
    for step in gors::scanner::Scanner::new(&cmd.file, &buffer) {
        serde_json::to_writer(&mut w, &step?)?;
        w.write_all(b"\n")?;
    }
    w.flush()?;

    Ok(())
}

struct RustcArgs<'a> {
    src: &'a str,
    out: Option<&'a str>,
    emit: Option<&'a str>,
    release: bool,
}

impl<'a> IntoIterator for RustcArgs<'a> {
    type Item = &'a str;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let mut out = vec![self.src, "--edition=2021"];

        if let Some(e) = self.emit {
            out.extend(["--emit", e]);
        }

        if let Some(o) = self.out {
            out.extend(["-o", o]);
        }

        if self.release {
            out.extend([
                "-Ccodegen-units=1",
                "-Clto=fat",
                "-Copt-level=3",
                "-Ctarget-cpu=native",
            ]);
        }

        out.into_iter()
    }
}
