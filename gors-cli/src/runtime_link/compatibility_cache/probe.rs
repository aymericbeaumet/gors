//! Injectable command boundary for resolving and querying one native rustc.

use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug)]
pub(in crate::runtime_link) struct ProbeError {
    message: String,
}

impl ProbeError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ProbeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProbeError {}

pub(super) trait CompatibilityProbe: Sync {
    fn resolve_rustc(&self, selector: &str) -> Result<PathBuf, ProbeError>;

    fn rustc_verbose_version(&self, rustc: &Path) -> Result<Vec<u8>, ProbeError>;

    fn target_libdir(&self, rustc: &Path, target: &str) -> Result<PathBuf, ProbeError>;
}

pub(super) struct SystemProbe;

impl CompatibilityProbe for SystemProbe {
    fn resolve_rustc(&self, selector: &str) -> Result<PathBuf, ProbeError> {
        let output = Command::new("rustup")
            .args(["which", "--toolchain", selector, "rustc"])
            .output()
            .map_err(|error| {
                ProbeError::new(format!(
                    "failed to run rustup which for toolchain {selector}: {error}"
                ))
            })?;
        successful_path_output(
            output,
            &format!("rustup which --toolchain {selector} rustc"),
        )
    }

    fn rustc_verbose_version(&self, rustc: &Path) -> Result<Vec<u8>, ProbeError> {
        let output = Command::new(rustc).arg("-vV").output().map_err(|error| {
            ProbeError::new(format!(
                "failed to run resolved rustc {} -vV: {error}",
                rustc.display()
            ))
        })?;
        ensure_success(output, &format!("{} -vV", rustc.display()))
    }

    fn target_libdir(&self, rustc: &Path, target: &str) -> Result<PathBuf, ProbeError> {
        let output = Command::new(rustc)
            .args(["--target", target, "--print", "target-libdir"])
            .output()
            .map_err(|error| {
                ProbeError::new(format!(
                    "failed to query target-libdir from resolved rustc {}: {error}",
                    rustc.display()
                ))
            })?;
        successful_path_output(
            output,
            &format!(
                "{} --target {target} --print target-libdir",
                rustc.display()
            ),
        )
    }
}

fn ensure_success(output: Output, command: &str) -> Result<Vec<u8>, ProbeError> {
    if !output.status.success() {
        return Err(ProbeError::new(format!(
            "{command} failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if output.stdout.is_empty() {
        return Err(ProbeError::new(format!("{command} returned empty stdout")));
    }
    Ok(output.stdout)
}

fn successful_path_output(output: Output, command: &str) -> Result<PathBuf, ProbeError> {
    let stdout = ensure_success(output, command)?;
    let mut path = std::str::from_utf8(&stdout)
        .map_err(|_| ProbeError::new(format!("{command} returned a non-UTF-8 path")))?;
    if let Some(without_newline) = path.strip_suffix('\n') {
        path = without_newline;
    }
    if let Some(without_carriage_return) = path.strip_suffix('\r') {
        path = without_carriage_return;
    }
    if path.is_empty() || path.contains(['\n', '\r', '\0']) {
        return Err(ProbeError::new(format!(
            "{command} returned an empty or ambiguous path"
        )));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(ProbeError::new(format!(
            "{command} returned a non-absolute path: {}",
            path.display()
        )));
    }
    Ok(path)
}
