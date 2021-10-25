use clap::Parser;

mod ast;
mod parser;

#[derive(Parser)]
#[clap(version = "1.0", author = "Aymeric Beaumet <hi@aymericbeaumet.com>")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(about = "Transpile Go source code to Rust")]
    Parse(Parse),
}

#[derive(Parser)]
struct Parse {
    #[clap(name = "file", about = "The file to transpile")]
    filepath: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Parse(cmd) => parse(cmd),
    }
}

fn parse(cmd: Parse) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(cmd.filepath)?;
    let file = parser::parse(&buffer)?;
    println!("{:?}", file);
    Ok(())
}
