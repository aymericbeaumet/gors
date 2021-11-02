#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
    #[clap(about = "Parse Go code and print the tokens")]
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
        stdout.write_all(&[b'\n'])?;
    }

    Ok(())
}
