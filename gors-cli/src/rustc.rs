use crate::timings::TimingCollector;
use gors_runtime_abi::NATIVE_RUNTIME_RUST_TOOLCHAIN;
use std::path::Path;
use std::process::Command;

pub const RUST_EDITION: &str = gors_runtime_abi::RUST_RUNTIME_EDITION;

pub fn compile_generated_binary(
    cache_dir: &Path,
    bin_path: &Path,
    runtime_path: &Path,
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
    let runtime_str = runtime_path.to_string_lossy();
    let rustc_args = RustcArgs {
        src: &src_str,
        runtime: &runtime_str,
        out: Some(&pending_str),
        emit: None,
        release,
    };

    let rustc_timer = timings.phase("cli.rustc");
    let rustc_status = Command::new("rustup")
        .args(["run", NATIVE_RUNTIME_RUST_TOOLCHAIN, "rustc"])
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

pub struct RustcArgs<'a> {
    pub src: &'a str,
    pub runtime: &'a str,
    pub out: Option<&'a str>,
    pub emit: Option<&'a str>,
    pub release: bool,
}

impl From<RustcArgs<'_>> for Vec<String> {
    fn from(args: RustcArgs<'_>) -> Self {
        let mut flags = vec![
            args.src.to_string(),
            format!("--edition={RUST_EDITION}"),
            "--extern".to_string(),
            format!(
                "{}={}",
                gors_runtime_abi::RUST_RUNTIME_CRATE_NAME,
                args.runtime
            ),
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

impl IntoIterator for RustcArgs<'_> {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        Vec::from(self).into_iter()
    }
}
