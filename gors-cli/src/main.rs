// Clippy lints are configured at workspace level in the root Cargo.toml

mod build;
mod cache;
mod cache_paths;
mod compiler;
mod diagnostics;
mod emit_rust;
mod help;
mod inspect;
mod options;
mod output;
mod program;
mod public_executable;
mod run;
mod runtime_descriptor;
mod runtime_link;
mod rustc;
mod timings;

use build::build;
use clap::Parser;
use emit_rust::emit_rust;
use help::{help, version};
use inspect::{ast, tokens};
use options::{Opts, SubCommand};
use run::run;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();

    let opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Ast(cmd) => ast(cmd),
        SubCommand::Build(cmd) => build(cmd),
        SubCommand::EmitRust(cmd) => emit_rust(cmd),
        SubCommand::Help(cmd) => help(cmd),
        SubCommand::Run(cmd) => run(cmd),
        SubCommand::Tokens(cmd) => tokens(cmd),
        SubCommand::Version => version(),
    }
}

#[cfg(test)]
use build::build_with_cache_base;
#[cfg(test)]
use cache::{
    CacheAccessLock, CliCacheManifest, FileArtifact, GeneratedOutputManifest,
    GeneratedRustIdentity, GeneratedRustIdentityOptions, InputSnapshot, generated_file_hashes,
    maybe_prune_cli_cache,
};
#[cfg(test)]
use compiler::cli_workspace;
#[cfg(test)]
use emit_rust::emit_rust_with_cache_base;
#[cfg(test)]
use gors::compiler::input::WorkspaceKey;
#[cfg(test)]
use options::Build;
#[cfg(test)]
use options::EmitRust;
#[cfg(test)]
use output::{
    OutputDirectoryLock, prepare_atomic_write, write_generated_export, write_generated_output,
    write_generated_output_locked,
};
#[cfg(test)]
use rustc::{ExecutableProduct, RustcAction, RustcProfile, TerminalToolchain};
#[cfg(test)]
use std::num::NonZeroUsize;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;

#[cfg(test)]
mod tests;
