#![allow(clippy::expect_used)]

use super::*;
use crate::cache::GeneratedRustIdentityOptions;

#[test]
fn every_program_command_uses_the_single_program_namespace() {
    let cache_base = PathBuf::from("/cache");
    let source_paths = vec!["main.go".to_string()];
    let identity = GeneratedRustIdentity::new(GeneratedRustIdentityOptions {
        source_paths: &source_paths,
    })
    .expect("generated Rust identity");

    assert_eq!(
        program_cache_dir(&cache_base, &identity),
        cache_base.join("programs").join(identity.fingerprint())
    );
}
