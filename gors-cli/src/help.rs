use crate::options::{Help, Opts};
use clap::CommandFactory;

pub fn help(cmd: Help) -> Result<(), Box<dyn std::error::Error>> {
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

pub fn version() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", version_line());
    Ok(())
}

fn version_line() -> String {
    format!(
        "gors version gors{} {} {}/{} runtime-contract={}",
        env!("CARGO_PKG_VERSION"),
        gors::STDLIB_VERSION,
        go_target_os(),
        go_target_arch(),
        gors::compiler::db::RuntimeAbiId::current(),
    )
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

#[cfg(test)]
mod tests;
