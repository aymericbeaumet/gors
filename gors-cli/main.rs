use clap::Parser;
use std::io::Write;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Compile(cmd) => compile(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
    }
}

#[derive(Parser)]
#[clap(version = "1.0", author = "Aymeric Beaumet <hi@aymericbeaumet.com>")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(about = "Parse the named Go file and print the AST")]
    Ast(Ast),
    #[clap(about = "Compile the named Go file and build an executable")]
    Build(Build),
    #[clap(about = "Compile the named Go file and print the generated Rust code")]
    Compile(Compile),
    #[clap(about = "Compile and run the named Go file")]
    Run(Run),
    #[clap(about = "Scan the named Go file and print the tokens")]
    Tokens(Tokens),
}

#[derive(Parser)]
struct Ast {
    #[clap(name = "file", about = "The file to parse")]
    filepath: String,
}

#[derive(Parser)]
struct Build {
    #[clap(name = "file", about = "The file to build")]
    filepath: String,
}

#[derive(Parser)]
struct Compile {
    #[clap(name = "file", about = "The file to compile")]
    filepath: String,
}

#[derive(Parser)]
struct Run {
    #[clap(name = "file", about = "The file to run")]
    filepath: String,
}

#[derive(Parser)]
struct Tokens {
    #[clap(name = "file", about = "The file to lex")]
    filepath: String,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let arena = gors::parser::Arena::new();
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let file = gors::parser::parse_file(&arena, &cmd.filepath, &buffer)?;
    gors::ast::fprint(&mut w, file)?;

    w.flush()?;
    Ok(())
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir::TempDir::new("gors")?;
    let main_file = dir.path().join("main.rs");
    let mut w = std::fs::File::create(&main_file)?;

    let arena = gors::parser::Arena::new();
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let parsed = gors::parser::parse_file(&arena, &cmd.filepath, &buffer)?;
    let compiled = gors::compiler::compile(parsed)?;
    gors::codegen::fprint(&mut w, compiled)?;
    w.sync_all()?;

    let rustc = Command::new("rustc")
        .args([
            main_file.to_str().unwrap(),
            "--edition=2021",
            "-Ccodegen-units=1",
            "-Clto=fat",
            "-Copt-level=3",
            "-Ctarget-cpu=native",
            "-o",
            "main",
        ])
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    Ok(())
}

fn compile(cmd: Compile) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let arena = gors::parser::Arena::new();
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let parsed = gors::parser::parse_file(&arena, &cmd.filepath, &buffer)?;
    let compiled = gors::compiler::compile(parsed)?;
    gors::codegen::fprint(&mut w, compiled)?;

    w.flush()?;
    Ok(())
}

fn run(cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir::TempDir::new("gors")?;
    let main_file = dir.path().join("main.rs");
    let bin_file = dir.path().join("main");
    let mut w = std::fs::File::create(&main_file)?;

    let arena = gors::parser::Arena::new();
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let parsed = gors::parser::parse_file(&arena, &cmd.filepath, &buffer)?;
    let compiled = gors::compiler::compile(parsed)?;
    gors::codegen::fprint(&mut w, compiled)?;
    w.sync_all()?;

    let rustc = Command::new("rustc")
        .args([
            main_file.to_str().unwrap(),
            "--edition=2021",
            "-o",
            bin_file.to_str().unwrap(),
        ])
        .output()?;
    if !rustc.status.success() {
        print!("{}", String::from_utf8_lossy(&rustc.stdout));
        eprint!("{}", String::from_utf8_lossy(&rustc.stderr));
        return Ok(());
    }

    let mut cmd = Command::new(&bin_file).spawn()?;
    cmd.wait()?;

    Ok(())
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let mut s = gors::scanner::Scanner::new(&cmd.filepath, &buffer);

    loop {
        let (pos, tok, lit) = s.scan()?;

        serde_json::to_writer(&mut w, &(pos, tok, lit))?;
        w.write_all(b"\n")?;

        if tok == gors::token::Token::EOF {
            break;
        }
    }

    w.flush()?;
    Ok(())
}
