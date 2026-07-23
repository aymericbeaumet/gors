//! Target-neutral identity for generated Rust source products.

use sha2::{Digest, Sha256};
use std::path::Path;

use super::{file_hash, normalized_path};

/// Explicit schema for CLI-owned source selection and compiler invocation.
///
/// CLI implementation details are intentionally not hashed. Bump this only
/// when the driver changes which semantic program is presented to `gors`.
pub const GENERATED_DRIVER_SCHEMA: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedRustIdentity {
    fingerprint: String,
}

pub struct GeneratedRustIdentityOptions<'a> {
    pub source_paths: &'a [String],
}

impl GeneratedRustIdentity {
    pub fn new(
        options: GeneratedRustIdentityOptions<'_>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_facts(
            options,
            gors::GENERATED_RUST_FINGERPRINT,
            GENERATED_DRIVER_SCHEMA,
        )
    }

    fn new_with_facts(
        options: GeneratedRustIdentityOptions<'_>,
        generated_rust_fingerprint: &str,
        driver_schema: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut hasher = Sha256::new();
        hash_part(&mut hasher, b"gors-generated-rust-request-v2");
        hash_part(&mut hasher, &driver_schema.to_le_bytes());
        hash_part(&mut hasher, generated_rust_fingerprint.as_bytes());
        hash_part(&mut hasher, gors::GO_VERSION.as_bytes());
        hash_part(&mut hasher, gors::STDLIB_VERSION.as_bytes());
        hash_part(
            &mut hasher,
            gors_runtime_abi::RuntimeAbiManifest::current()
                .identity()
                .as_bytes(),
        );

        let mut source_facts = options
            .source_paths
            .iter()
            .map(|source_path| {
                Ok::<_, Box<dyn std::error::Error>>((
                    normalized_path(Path::new(source_path))?,
                    module_context(Path::new(source_path))?,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        source_facts.sort();
        for (source_path, module_context) in source_facts {
            hash_part(&mut hasher, source_path.as_bytes());
            hash_part(&mut hasher, module_context.as_bytes());
        }

        Ok(Self {
            fingerprint: hex_digest(hasher.finalize()),
        })
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[cfg(test)]
    pub(super) fn for_test(fingerprint: impl Into<String>) -> Self {
        Self {
            fingerprint: fingerprint.into(),
        }
    }
}

fn module_context(source_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut directory = if source_path.is_dir() {
        source_path.to_path_buf()
    } else {
        source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    if directory.is_relative() {
        directory = std::env::current_dir()?.join(directory);
    }
    loop {
        let go_mod = directory.join("go.mod");
        if go_mod.is_file() {
            return Ok(format!(
                "{}:{}",
                normalized_path(&go_mod)?,
                file_hash(&go_mod)?
            ));
        }
        if !directory.pop() {
            return Ok("no-go-mod".to_string());
        }
    }
}

fn hash_part(hasher: &mut Sha256, part: &[u8]) {
    hasher.update(part.len().to_le_bytes());
    hasher.update(part);
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn source_fixture() -> (tempfile::TempDir, Vec<String>) {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("main.go");
        std::fs::write(&source, "package main\n").unwrap();
        (directory, vec![source.to_string_lossy().into_owned()])
    }

    #[test]
    fn identity_tracks_generated_schema_and_driver_selection() {
        let (_directory, source_paths) = source_fixture();
        let options = || GeneratedRustIdentityOptions {
            source_paths: &source_paths,
        };
        let baseline = GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 1).unwrap();
        let same = GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 1).unwrap();
        let compiler = GeneratedRustIdentity::new_with_facts(options(), "compiler-b", 1).unwrap();
        let driver = GeneratedRustIdentity::new_with_facts(options(), "compiler-a", 2).unwrap();

        assert_eq!(baseline, same);
        assert_ne!(baseline, compiler);
        assert_ne!(baseline, driver);
    }

    #[test]
    fn identity_tracks_explicit_module_context() {
        let (directory, source_paths) = source_fixture();
        let module = directory.path().join("go.mod");
        std::fs::write(&module, "module example.com/first\n").unwrap();
        let baseline = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &source_paths,
            },
            "compiler",
            1,
        )
        .unwrap();

        std::fs::write(&module, "module example.com/second\n").unwrap();
        let changed = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &source_paths,
            },
            "compiler",
            1,
        )
        .unwrap();

        assert_ne!(baseline, changed);
    }

    #[test]
    fn identity_tracks_explicit_source_selection() {
        let (directory, source_paths) = source_fixture();
        let alternate = directory.path().join("alternate.go");
        std::fs::write(&alternate, "package main\n").unwrap();
        let alternate = alternate.to_string_lossy().into_owned();
        let baseline = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &source_paths,
            },
            "compiler",
            1,
        )
        .unwrap();
        let changed = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &[alternate],
            },
            "compiler",
            1,
        )
        .unwrap();

        assert_ne!(baseline, changed);
    }

    #[test]
    fn identity_canonicalizes_equivalent_source_spellings() {
        let (directory, source_paths) = source_fixture();
        let alternate = directory
            .path()
            .join(".")
            .join("main.go")
            .to_string_lossy()
            .into_owned();
        let canonical = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &source_paths,
            },
            "compiler",
            1,
        )
        .unwrap();
        let equivalent = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &[alternate],
            },
            "compiler",
            1,
        )
        .unwrap();

        assert_eq!(canonical, equivalent);
    }

    #[test]
    fn identity_is_independent_from_source_argument_order() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.go");
        let second = directory.path().join("second.go");
        std::fs::write(&first, "package main\n").unwrap();
        std::fs::write(&second, "package main\n").unwrap();
        let first = first.to_string_lossy().into_owned();
        let second = second.to_string_lossy().into_owned();
        let forward = vec![first.clone(), second.clone()];
        let reverse = vec![second, first];

        let forward = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &forward,
            },
            "compiler",
            1,
        )
        .unwrap();
        let reverse = GeneratedRustIdentity::new_with_facts(
            GeneratedRustIdentityOptions {
                source_paths: &reverse,
            },
            "compiler",
            1,
        )
        .unwrap();

        assert_eq!(forward, reverse);
    }
}
