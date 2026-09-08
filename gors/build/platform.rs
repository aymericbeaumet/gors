//! Explicit translation between Rust host/target triples and Go platforms.
//!
//! The host selects the downloadable Go SDK archive. The Rust compilation
//! target independently selects the Go source files and build constraints that
//! are embedded as resolver metadata.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoPlatform {
    pub os: &'static str,
    pub arch: &'static str,
}

pub fn host_sdk_platform(rust_os: &str, rust_arch: &str) -> Result<GoPlatform, String> {
    let os = match rust_os {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        unsupported => {
            return Err(format!(
                "unsupported host OS for Go SDK download: {unsupported}"
            ));
        }
    };
    let arch = match rust_arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        unsupported => {
            return Err(format!(
                "unsupported host architecture for Go SDK download: {unsupported}"
            ));
        }
    };
    Ok(GoPlatform { os, arch })
}

pub fn target_source_platform(rust_os: &str, rust_arch: &str) -> Result<GoPlatform, String> {
    match (rust_os, rust_arch) {
        // wasm32-unknown-unknown is the browser compiler target. Go names its
        // corresponding build-selection platform js/wasm.
        ("unknown", "wasm32") => Ok(GoPlatform {
            os: "js",
            arch: "wasm",
        }),
        // Keep the mapping explicit for Rust's WASI spellings as well.
        ("wasi" | "wasip1", "wasm32") => Ok(GoPlatform {
            os: "wasip1",
            arch: "wasm",
        }),
        (rust_os, rust_arch) => {
            let os = match rust_os {
                "macos" => "darwin",
                "linux" => "linux",
                "windows" => "windows",
                unsupported => {
                    return Err(format!(
                        "unsupported Rust target OS for Go source metadata: {unsupported}"
                    ));
                }
            };
            let arch = match rust_arch {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                unsupported => {
                    return Err(format!(
                        "unsupported Rust target architecture for Go source metadata: {unsupported}"
                    ));
                }
            };
            Ok(GoPlatform { os, arch })
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn host_platform_selects_the_downloadable_sdk() {
        assert_eq!(
            host_sdk_platform("macos", "x86_64"),
            Ok(GoPlatform {
                os: "darwin",
                arch: "amd64"
            })
        );
        assert_eq!(
            host_sdk_platform("linux", "aarch64"),
            Ok(GoPlatform {
                os: "linux",
                arch: "arm64"
            })
        );
    }

    #[test]
    fn windows_hosts_and_targets_select_both_release_architectures() {
        for (rust_arch, go_arch) in [("x86_64", "amd64"), ("aarch64", "arm64")] {
            let expected = Ok(GoPlatform {
                os: "windows",
                arch: go_arch,
            });
            assert_eq!(host_sdk_platform("windows", rust_arch), expected);
            assert_eq!(target_source_platform("windows", rust_arch), expected);
        }
    }

    #[test]
    fn target_platform_is_independent_from_the_host() {
        let host = host_sdk_platform("linux", "x86_64").unwrap();
        let target = target_source_platform("linux", "aarch64").unwrap();

        assert_eq!(host.arch, "amd64");
        assert_eq!(target.arch, "arm64");
    }

    #[test]
    fn browser_and_wasi_targets_use_go_wasm_build_tags() {
        assert_eq!(
            target_source_platform("unknown", "wasm32"),
            Ok(GoPlatform {
                os: "js",
                arch: "wasm"
            })
        );
        assert_eq!(
            target_source_platform("wasi", "wasm32"),
            Ok(GoPlatform {
                os: "wasip1",
                arch: "wasm"
            })
        );
    }

    #[test]
    fn unsupported_hosts_and_targets_fail_explicitly() {
        assert!(host_sdk_platform("freebsd", "x86_64").is_err());
        assert!(host_sdk_platform("linux", "riscv64").is_err());
        assert!(target_source_platform("freebsd", "x86_64").is_err());
        assert!(target_source_platform("linux", "riscv64").is_err());
        assert!(target_source_platform("unknown", "aarch64").is_err());
    }
}
