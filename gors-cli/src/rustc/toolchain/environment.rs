use std::collections::BTreeMap;
use std::path::Path;

use super::{
    PlatformLinkContract, TerminalToolchainError, ToolArtifact, canonical_deployment_target_value,
    is_apple_darwin_target, is_windows_msvc_target,
};

pub(super) fn deterministic_environment(
    rustc: &ToolArtifact,
    linker: &ToolArtifact,
) -> Result<BTreeMap<String, String>, TerminalToolchainError> {
    let mut path_entries = Vec::new();
    for tool in [rustc, linker] {
        if let Some(parent) = Path::new(&tool.path).parent() {
            let parent = parent.to_string_lossy().into_owned();
            if !path_entries.contains(&parent) {
                path_entries.push(parent);
            }
        }
    }
    #[cfg(unix)]
    for fallback in ["/usr/bin", "/bin"] {
        if !path_entries.iter().any(|entry| entry == fallback) {
            path_entries.push(fallback.to_string());
        }
    }
    #[cfg(windows)]
    if let Some(ambient_path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&ambient_path) {
            let directory = directory.into_os_string().into_string().map_err(|_| {
                TerminalToolchainError::LinkerResolution(
                    "terminal PATH contains a non-UTF-8 directory".to_string(),
                )
            })?;
            if !path_entries.contains(&directory) {
                path_entries.push(directory);
            }
        }
    }
    let path = std::env::join_paths(path_entries.iter().map(Path::new))
        .map_err(|error| {
            TerminalToolchainError::LinkerResolution(format!(
                "cannot construct deterministic terminal PATH: {error}"
            ))
        })?
        .into_string()
        .map_err(|_| {
            TerminalToolchainError::LinkerResolution(
                "deterministic terminal PATH is not valid UTF-8".to_string(),
            )
        })?;
    let environment = BTreeMap::from([
        ("LANG".to_string(), "C".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
        ("PATH".to_string(), path),
        ("SOURCE_DATE_EPOCH".to_string(), "0".to_string()),
        ("ZERO_AR_DATE".to_string(), "1".to_string()),
    ]);
    #[cfg(windows)]
    {
        let mut environment = environment;
        for name in [
            "SystemRoot",
            "LIB",
            "LIBPATH",
            "INCLUDE",
            "VCToolsInstallDir",
            "WindowsSdkDir",
            "WindowsSDKVersion",
            "UCRTVersion",
        ] {
            if let Some(value) = std::env::var_os(name) {
                let value = value.into_string().map_err(|_| {
                    TerminalToolchainError::LinkerResolution(format!(
                        "terminal environment variable {name} is not valid UTF-8"
                    ))
                })?;
                environment.insert(name.to_string(), value);
            }
        }
        Ok(environment)
    }
    #[cfg(not(windows))]
    {
        Ok(environment)
    }
}

pub(super) fn is_canonical_environment(
    environment: &BTreeMap<String, String>,
    target: &str,
    platform: Option<&PlatformLinkContract>,
) -> bool {
    for (name, expected) in [
        ("LANG", "C"),
        ("LC_ALL", "C"),
        ("SOURCE_DATE_EPOCH", "0"),
        ("ZERO_AR_DATE", "1"),
    ] {
        if environment.get(name).map(String::as_str) != Some(expected) {
            return false;
        }
    }
    let Some(path) = environment.get("PATH") else {
        return false;
    };
    let mut path_entries = Vec::new();
    for entry in std::env::split_paths(path) {
        if !entry.is_absolute() || path_entries.contains(&entry) {
            return false;
        }
        path_entries.push(entry);
    }
    if path_entries.is_empty() {
        return false;
    }
    if environment
        .get("MACOSX_DEPLOYMENT_TARGET")
        .is_some_and(|value| !canonical_deployment_target_value(value))
    {
        return false;
    }
    environment.iter().all(|(name, value)| {
        !name.is_empty()
            && !name.contains(['=', '\0'])
            && !value.contains('\0')
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            && allowed_environment_name(name, target, platform)
    })
}

fn allowed_environment_name(
    name: &str,
    target: &str,
    platform: Option<&PlatformLinkContract>,
) -> bool {
    if matches!(
        name,
        "LANG" | "LC_ALL" | "PATH" | "SOURCE_DATE_EPOCH" | "ZERO_AR_DATE"
    ) {
        return true;
    }
    if is_apple_darwin_target(target) && platform.is_some() {
        return matches!(name, "SDKROOT" | "MACOSX_DEPLOYMENT_TARGET");
    }
    is_windows_msvc_target(target)
        && matches!(
            name,
            "SystemRoot"
                | "LIB"
                | "LIBPATH"
                | "INCLUDE"
                | "VCToolsInstallDir"
                | "WindowsSdkDir"
                | "WindowsSDKVersion"
                | "UCRTVersion"
        )
}
