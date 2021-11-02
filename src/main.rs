mod scanner;
mod token;

use clap::Parser;
use mimalloc::MiMalloc;
use std::io::Write;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[clap(version = "1.0", author = "Aymeric Beaumet <hi@aymericbeaumet.com>")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(about = "Tokens Golang code and print the AST")]
    Tokens(Tokens),
}

#[derive(Parser)]
struct Tokens {
    #[clap(name = "file", about = "The file to parse")]
    filepath: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Tokens(cmd) => tokens(cmd),
    }
}

fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(&cmd.filepath)?;
    let tokens = scanner::scan(&cmd.filepath, &buffer)?;

    let mut stdout = std::io::stdout();
    for token in tokens {
        serde_json::to_writer(&stdout, &token)?;
        stdout.write(&[b'\n'])?;
    }

    Ok(())
}
