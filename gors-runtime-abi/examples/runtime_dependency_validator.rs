use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use gors_runtime_abi::{CURRENT_RUNTIME_DEPENDENCY_SCHEMA, RuntimeRequirement};

const MAX_RUNTIME_DEPENDENCY_BYTES: usize = 4 * 1024;
const MAX_RUNTIME_DEPENDENCY_READ_BYTES: u64 = 4 * 1024 + 1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 4 {
        return Err(std::io::Error::other(
            "usage: runtime_dependency_validator <request.json> <schema> <contract> <supported-operation-ids>",
        )
        .into());
    }
    let mut request = Vec::with_capacity(MAX_RUNTIME_DEPENDENCY_BYTES.saturating_add(1));
    std::fs::File::open(argument_path(&arguments, 0)?)?
        .take(MAX_RUNTIME_DEPENDENCY_READ_BYTES)
        .read_to_end(&mut request)?;
    if request.len() > MAX_RUNTIME_DEPENDENCY_BYTES {
        return Err(std::io::Error::other(format!(
            "runtime dependency request exceeds {MAX_RUNTIME_DEPENDENCY_BYTES} bytes"
        ))
        .into());
    }
    let schema = argument_text(&arguments, 1, "schema")?
        .parse::<u32>()
        .map_err(|error| std::io::Error::other(format!("invalid schema: {error}")))?;
    if schema != CURRENT_RUNTIME_DEPENDENCY_SCHEMA {
        return Err(std::io::Error::other(format!(
            "provider dependency schema {schema} is unsupported; expected {CURRENT_RUNTIME_DEPENDENCY_SCHEMA}"
        ))
        .into());
    }
    let contract = argument_text(&arguments, 2, "contract")?;
    if !is_contract_identity(&contract) {
        return Err(std::io::Error::other(
            "provider contract must be 64 lowercase hexadecimal characters",
        )
        .into());
    }
    let supported =
        parse_supported_operations(&argument_text(&arguments, 3, "supported operation IDs")?)?;

    RequestParser::new(&request, schema, &contract, &supported).parse()?;
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

fn parse_supported_operations(value: &str) -> Result<BTreeSet<u16>, Box<dyn std::error::Error>> {
    let mut operations = BTreeSet::new();
    for component in value.split(',') {
        let operation = component.parse::<u16>().map_err(|error| {
            std::io::Error::other(format!(
                "invalid supported operation ID {component:?}: {error}"
            ))
        })?;
        if !operations.insert(operation) {
            return Err(std::io::Error::other(format!(
                "duplicate supported operation ID {operation}"
            ))
            .into());
        }
    }
    RuntimeRequirement::from_operation_ids(operations.iter().copied())?;
    Ok(operations)
}

fn is_contract_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct RequestParser<'a> {
    bytes: &'a [u8],
    offset: usize,
    expected_schema: u32,
    expected_contract: &'a str,
    supported_operations: &'a BTreeSet<u16>,
}

impl<'a> RequestParser<'a> {
    fn new(
        bytes: &'a [u8],
        expected_schema: u32,
        expected_contract: &'a str,
        supported_operations: &'a BTreeSet<u16>,
    ) -> Self {
        Self {
            bytes,
            offset: 0,
            expected_schema,
            expected_contract,
            supported_operations,
        }
    }

    fn parse(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.whitespace();
        self.byte(b'{')?;
        let mut schema_seen = false;
        let mut contract_seen = false;
        let mut operations_seen = false;
        loop {
            self.whitespace();
            if self.consume(b'}') {
                break;
            }
            let key = self.ascii_string()?;
            self.whitespace();
            self.byte(b':')?;
            self.whitespace();
            match key.as_str() {
                "schema_version" if !schema_seen => {
                    schema_seen = true;
                    let schema = self.unsigned()?;
                    if schema != u64::from(self.expected_schema) {
                        return self
                            .error(format!("runtime dependency schema {schema} is unsupported"));
                    }
                }
                "contract" if !contract_seen => {
                    contract_seen = true;
                    let contract = self.ascii_string()?;
                    if contract != self.expected_contract {
                        return self.error("runtime contract does not match provider".to_string());
                    }
                }
                "operation_ids" if !operations_seen => {
                    operations_seen = true;
                    self.operations()?;
                }
                "schema_version" | "contract" | "operation_ids" => {
                    return self.error(format!("duplicate request field {key:?}"));
                }
                _ => return self.error(format!("unknown request field {key:?}")),
            }
            self.whitespace();
            if self.consume(b',') {
                self.whitespace();
                if self.current() == Some(b'}') {
                    return self.error("trailing object comma is forbidden".to_string());
                }
                continue;
            }
            self.byte(b'}')?;
            break;
        }
        self.whitespace();
        if self.offset != self.bytes.len() {
            return self.error("trailing data after runtime request".to_string());
        }
        if !(schema_seen && contract_seen && operations_seen) {
            return self.error(
                "request must contain schema_version, contract, and operation_ids".to_string(),
            );
        }
        Ok(())
    }

    fn operations(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.byte(b'[')?;
        self.whitespace();
        let mut previous = None;
        if self.consume(b']') {
            return Ok(());
        }
        loop {
            let value = self.unsigned()?;
            let operation = u16::try_from(value)
                .map_err(|_| std::io::Error::other(format!("operation ID {value} exceeds u16")))?;
            RuntimeRequirement::from_operation_ids([operation])?;
            if !self.supported_operations.contains(&operation) {
                return self.error(format!("runtime operation ID {operation} is unsupported"));
            }
            if previous.is_some_and(|previous| previous >= operation) {
                return self.error("runtime operation IDs must be strictly ascending".to_string());
            }
            previous = Some(operation);
            self.whitespace();
            if self.consume(b',') {
                self.whitespace();
                continue;
            }
            self.byte(b']')?;
            return Ok(());
        }
    }

    fn ascii_string(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.byte(b'\"')?;
        let start = self.offset;
        while let Some(byte) = self.current() {
            if byte == b'\"' {
                let value = std::str::from_utf8(
                    self.bytes
                        .get(start..self.offset)
                        .ok_or_else(|| std::io::Error::other("invalid string range"))?,
                )?;
                if !value
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
                {
                    return self
                        .error("request strings must use unescaped printable ASCII".to_string());
                }
                self.offset = self.offset.saturating_add(1);
                return Ok(value.to_string());
            }
            if byte == b'\\' || byte < b' ' || !byte.is_ascii() {
                return self.error("request strings must use canonical ASCII".to_string());
            }
            self.offset = self.offset.saturating_add(1);
        }
        self.error("unterminated request string".to_string())
    }

    fn unsigned(&mut self) -> Result<u64, Box<dyn std::error::Error>> {
        let start = self.offset;
        while self.current().is_some_and(|byte| byte.is_ascii_digit()) {
            self.offset = self.offset.saturating_add(1);
        }
        let digits = self
            .bytes
            .get(start..self.offset)
            .ok_or_else(|| std::io::Error::other("invalid number range"))?;
        if digits.is_empty() || (digits.len() > 1 && digits.first() == Some(&b'0')) {
            return self.error("expected a canonical unsigned integer".to_string());
        }
        Ok(std::str::from_utf8(digits)?.parse()?)
    }

    fn whitespace(&mut self) {
        while self
            .current()
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.offset = self.offset.saturating_add(1);
        }
    }

    fn byte(&mut self, expected: u8) -> Result<(), Box<dyn std::error::Error>> {
        if self.consume(expected) {
            Ok(())
        } else {
            self.error(format!("expected byte {:?}", char::from(expected)))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.current() == Some(expected) {
            self.offset = self.offset.saturating_add(1);
            true
        } else {
            false
        }
    }

    fn current(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn error<T>(&self, message: String) -> Result<T, Box<dyn std::error::Error>> {
        Err(std::io::Error::other(format!(
            "invalid runtime dependency request at byte {}: {message}",
            self.offset
        ))
        .into())
    }
}
