use std::fmt::Write as _;
use std::path::Path;

use gors_runtime_abi::{
    CURRENT_RUNTIME_DEPENDENCY_SCHEMA, DataWidth, Endianness, ImplementationHash, ProducerIdentity,
    RUST_RUNTIME_CRATE_NAME, RuntimeAbiManifest, RuntimeArtifactFormat, RuntimeArtifactManifest,
    RuntimeOp, RustRlibCompatibility, RustRlibProducer, TargetCapabilities, TargetCapability,
    TargetModel, canonical_target_libdir_record,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 8 {
        return Err(std::io::Error::other(
            "usage: runtime_provider_manifest <producer-identity|auto> <rustc-vV> <target-libdir> <target-triple> <pointer-width> <endianness> <payload> <output>",
        )
        .into());
    }
    let producer_argument = argument_text(&arguments, 0, "producer identity")?;
    let rustc_verbose_version = std::fs::read(argument_path(&arguments, 1)?)?;
    let target_libdir_record = canonical_target_libdir_record(argument_path(&arguments, 2)?)?;
    let triple = argument_text(&arguments, 3, "target triple")?;
    let pointer_width = match argument_text(&arguments, 4, "pointer width")?.as_str() {
        "32" => DataWidth::Bits32,
        "64" => DataWidth::Bits64,
        value => {
            return Err(std::io::Error::other(format!(
                "unsupported pointer width {value:?}; expected 32 or 64"
            ))
            .into());
        }
    };
    let (endianness, endianness_name) = match argument_text(&arguments, 5, "endianness")?.as_str() {
        "little" => (Endianness::Little, "little"),
        "big" => (Endianness::Big, "big"),
        value => {
            return Err(std::io::Error::other(format!(
                "unsupported endianness {value:?}; expected little or big"
            ))
            .into());
        }
    };
    let payload = std::fs::read(argument_path(&arguments, 6)?)?;
    let output = argument_path(&arguments, 7)?;

    let target = TargetModel::new(triple.clone(), pointer_width, endianness)?;
    let producer_identity = if producer_argument == "auto" {
        RustRlibProducer::new(
            &rustc_verbose_version,
            target_libdir_record.clone(),
            target.clone(),
        )?
        .identity()
    } else {
        ProducerIdentity::from_bytes(parse_sha256(&producer_argument)?)
    };
    let compatibility =
        RustRlibCompatibility::new(&rustc_verbose_version, target_libdir_record, target.clone())?;
    let contract = RuntimeAbiManifest::current();
    let provided_capabilities = TargetCapabilities::new([TargetCapability::StandardIo]);
    let manifest = RuntimeArtifactManifest::new(
        contract.identity(),
        target,
        provided_capabilities.clone(),
        RuntimeArtifactFormat::RustRlibV1,
        compatibility.identity(),
        ImplementationHash::sha256(&payload),
    );
    let supported_operations = RuntimeOp::ALL.iter().copied().filter(|operation| {
        operation
            .required_capabilities()
            .iter()
            .all(|capability| provided_capabilities.contains(*capability))
    });

    let mut json = String::new();
    writeln!(json, "{{")?;
    writeln!(json, "  \"schema_version\": {},", manifest.schema().get())?;
    writeln!(
        json,
        "  \"runtime_dependency_schema_version\": {CURRENT_RUNTIME_DEPENDENCY_SCHEMA},"
    )?;
    writeln!(json, "  \"contract\": \"{}\",", manifest.contract())?;
    writeln!(json, "  \"target_triple\": {},", json_string(&triple))?;
    writeln!(
        json,
        "  \"target_pointer_width\": {},",
        pointer_width.bits()
    )?;
    writeln!(json, "  \"target_endianness\": \"{endianness_name}\",")?;
    writeln!(json, "  \"provided_capabilities\": [\"standard_io\"],")?;
    writeln!(json, "  \"format\": \"rust-rlib-v1\",")?;
    writeln!(json, "  \"extern_crate\": \"{RUST_RUNTIME_CRATE_NAME}\",")?;
    writeln!(
        json,
        "  \"rustc_release_record_hex\": \"{}\",",
        hex_bytes(compatibility.rustc_release_record())
    )?;
    writeln!(
        json,
        "  \"target_libdir_record_hex\": \"{}\",",
        hex_bytes(compatibility.target_libdir_record())
    )?;
    writeln!(json, "  \"producer_identity\": \"{producer_identity}\",")?;
    writeln!(
        json,
        "  \"compatibility_identity\": \"{}\",",
        manifest.compatibility()
    )?;
    writeln!(
        json,
        "  \"implementation_hash\": \"{}\",",
        manifest.implementation()
    )?;
    writeln!(
        json,
        "  \"artifact_identity\": \"{}\",",
        manifest.identity()
    )?;
    write!(json, "  \"supported_operation_ids\": [")?;
    for (index, operation) in supported_operations.enumerate() {
        if index != 0 {
            json.push_str(", ");
        }
        write!(json, "{}", operation.id().get())?;
    }
    writeln!(json, "]")?;
    writeln!(json, "}}")?;

    std::fs::write(output, json)?;
    Ok(())
}

fn argument_path(
    arguments: &[std::ffi::OsString],
    index: usize,
) -> Result<&Path, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .map(Path::new)
        .ok_or_else(|| std::io::Error::other(format!("missing argument {index}")).into())
}

fn argument_text(
    arguments: &[std::ffi::OsString],
    index: usize,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    arguments
        .get(index)
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| std::io::Error::other(format!("{name} must be valid UTF-8")).into())
}

fn json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                output.push_str("\\u00");
                let byte = u8::try_from(u32::from(character)).map_or(0, |value| value);
                output.push(hex_digit(byte >> 4));
                output.push(hex_digit(byte & 0x0f));
            }
            character => output.push(character),
        }
    }
    output.push('\"');
    output
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(hex_digit(byte >> 4));
        output.push(hex_digit(byte & 0x0f));
    }
    output
}

fn parse_sha256(value: &str) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    if value.len() != 64 {
        return Err(
            std::io::Error::other("producer identity must be 64 lowercase hex digits").into(),
        );
    }
    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let (high, low) = match chunk {
            [high, low] => (parse_hex_digit(*high)?, parse_hex_digit(*low)?),
            _ => return Err(std::io::Error::other("invalid producer identity width").into()),
        };
        let Some(byte) = bytes.get_mut(index) else {
            return Err(std::io::Error::other("producer identity exceeds SHA-256 width").into());
        };
        *byte = (high << 4) | low;
    }
    Ok(bytes)
}

fn parse_hex_digit(value: u8) -> Result<u8, Box<dyn std::error::Error>> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(std::io::Error::other("producer identity must use lowercase hex").into()),
    }
}

fn hex_digit(nibble: u8) -> char {
    char::from(if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble.saturating_sub(10)
    })
}
