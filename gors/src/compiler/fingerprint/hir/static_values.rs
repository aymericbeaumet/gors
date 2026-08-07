//! Canonical encoding for compiler-owned static aggregate values.

use super::super::encoder::{Encoder, const_value};
use crate::compiler::types::StaticValue;

pub(super) fn encode_static_value(encoder: &mut Encoder, value: &StaticValue) {
    match value {
        StaticValue::Constant(value) => {
            encoder.variant(b"constant", |encoder| const_value(encoder, value));
        }
        StaticValue::Struct(fields) => {
            encoder.variant(b"struct", |encoder| {
                encoder.sequence(fields, encode_static_value);
            });
        }
        StaticValue::Array(elements) => {
            encoder.variant(b"array", |encoder| {
                encoder.sequence(elements, encode_static_value);
            });
        }
        StaticValue::Slice(elements) => {
            encoder.variant(b"slice", |encoder| {
                encoder.sequence(elements, encode_static_value);
            });
        }
    }
}
