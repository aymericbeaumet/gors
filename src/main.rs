mod ast;
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
    #[clap(about = "Parse Go code and print the AST")]
    Ast(Ast),
    #[clap(about = "Parse Go code and print the tokens")]
    Tokens(Tokens),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
    }
}

#[derive(Parser)]
struct Ast {
    #[clap(name = "file", about = "The file to parse")]
    filepath: String,
}

fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let file = parser::parse_file(&cmd.filepath, &buffer)?;
    let mut stdout = std::io::stdout();

    ast::fprint(&mut stdout, &file)?;

    Ok(())
}

#[derive(Parser)]
struct Tokens {
    #[clap(name = "file", about = "The file to lex")]
    filepath: String,
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let mut s = scanner::Scanner::new(&cmd.filepath, &buffer);
    let mut stdout = std::io::stdout();

    loop {
        let (pos, tok, lit) = s.scan()?;

        serde_json::to_writer(&stdout, &(pos, tok, lit))?;
        stdout.write_all(&[b'\n'])?;

        if tok == token::Token::EOF {
            break;
        }
    }

    Ok(())
}
