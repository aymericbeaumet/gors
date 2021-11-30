mod ast;
mod codegen;
mod parser;
mod scanner;
mod token;

use clap::Parser;
use std::io::Write;

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
    #[clap(about = "Compile the named Go file")]
    Build(Build),
    #[clap(about = "Compile and run the named Go file")]
    Run(Run),
    #[clap(about = "Scan the named Go file and print the tokens")]
    Tokens(Tokens),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
        SubCommand::Run(cmd) => run(cmd),
    }
}

#[derive(Parser)]
struct Ast {
    #[clap(name = "file", about = "The file to parse")]
    filepath: String,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let arena = parser::Arena::new();
    let filepath = cmd.filepath;
    let buffer = std::fs::read_to_string(&filepath)?;

    let file = parser::parse_file(&arena, &filepath, &buffer)?;

    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    ast::fprint(&mut w, file)?;

    w.flush()?;
    Ok(())
}

#[derive(Parser)]
struct Build {
    #[clap(name = "file", about = "The file to build")]
    filepath: String,
}

fn build(cmd: Build) -> Result<(), Box<dyn std::error::Error>> {
    let arena = parser::Arena::new();
    let filepath = cmd.filepath;
    let buffer = std::fs::read_to_string(&filepath)?;

    let file = parser::parse_file(&arena, &filepath, &buffer)?;

    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    codegen::fprint(&mut w, file)?;

    w.flush()?;
    Ok(())
}

#[derive(Parser)]
struct Run {
    #[clap(name = "file", about = "The file to run")]
    filepath: String,
}

fn run(_cmd: Run) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[derive(Parser)]
struct Tokens {
    #[clap(name = "file", about = "The file to lex")]
    filepath: String,
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let filepath = cmd.filepath;
    let buffer = std::fs::read_to_string(&filepath)?;

    let mut s = scanner::Scanner::new(&filepath, &buffer);

    let stdout = std::io::stdout();
    let mut w = std::io::BufWriter::with_capacity(8192, stdout.lock());

    loop {
        let (pos, tok, lit) = s.scan()?;

        serde_json::to_writer(&mut w, &(pos, tok, lit))?;
        w.write_all(b"\n")?;

        if tok == token::Token::EOF {
            break;
        }
    }

    w.flush()?;
    Ok(())
}
