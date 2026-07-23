//! Canonical fingerprints for immutable compiler stage products.
//!
//! These digests are suitable red-green query keys because they are computed
//! from an explicit, schema-versioned binary encoding. They deliberately do
//! not use `Debug`, generated Rust text, JSON, or Rust's unstable `Hash`
//! implementation. A fingerprint is only a compact equality accelerator;
//! persistent caches must still validate their schema and full cache key.

mod encoder;
mod hir;
mod mir;
mod root;
mod rust_ir;

use std::fmt;

/// SHA-256 digest of one canonically encoded compiler stage product.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Return the digest bytes without exposing a mutable representation.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Return a stable lowercase hexadecimal spelling.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(hex_digit(byte >> 4));
            output.push(hex_digit(byte & 0x0f));
        }
        output
    }

    #[must_use]
    pub(in crate::compiler) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0 => '0',
        1 => '1',
        2 => '2',
        3 => '3',
        4 => '4',
        5 => '5',
        6 => '6',
        7 => '7',
        8 => '8',
        9 => '9',
        10 => 'a',
        11 => 'b',
        12 => 'c',
        13 => 'd',
        14 => 'e',
        _ => 'f',
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

pub use hir::{hir_constant, hir_file, hir_function};
pub use mir::{mir_file, mir_function};
pub(in crate::compiler) use rust_ir::runtime_requirement;
pub use rust_ir::{rust_ir_file, rust_ir_function};

pub(in crate::compiler) use root::rust_ir_root_inputs;

/// Fingerprint an ordered list of already-canonical byte parts.
///
/// This compiler-internal helper lets query input/product models share the
/// stage digest type and encoding envelope without exposing the full encoder.
#[must_use]
pub(in crate::compiler) fn fingerprint_parts(domain: &[u8], parts: &[&[u8]]) -> Fingerprint {
    let mut encoder = encoder::Encoder::root(domain);
    encoder.field(b"parts", |encoder| {
        encoder.sequence(parts, |encoder, part| encoder.blob(part));
    });
    encoder.finish()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::unwrap_used
)]
mod tests;
