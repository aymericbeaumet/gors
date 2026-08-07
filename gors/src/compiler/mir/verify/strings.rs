//! MIR verification for Go string conversions, indexing, slicing, and range helpers.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{IntTy, Ty, UintTy};

pub(super) fn is_string_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::StringFromRune
            | hir::Builtin::StringFromSliceU8
            | hir::Builtin::StringFromSliceRunes
            | hir::Builtin::StringToSliceRunes
            | hir::Builtin::StringLen
            | hir::Builtin::StringIndex
            | hir::Builtin::StringRange
            | hir::Builtin::StringRangeCount
            | hir::Builtin::StringRangeIndexAt
            | hir::Builtin::StringRangeRuneAt
    )
}

pub(super) fn verify_string_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    match builtin {
        hir::Builtin::StringFromRune => {
            let [argument] = arguments else {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR rune conversion arguments: {arguments:?}"
                )));
            };
            if !argument.is_integer() {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR rune conversion argument: {argument:?}"
                )));
            }
            verify_single_destination(destinations, is_string, "rune conversion")
        }
        hir::Builtin::StringFromSliceU8 => {
            if !matches!(arguments, [argument] if is_byte_slice(argument)) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR byte slice conversion arguments: {arguments:?}"
                )));
            }
            verify_single_destination(destinations, is_string, "byte slice conversion")
        }
        hir::Builtin::StringFromSliceRunes => {
            if !matches!(arguments, [argument] if is_rune_slice(argument)) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR rune slice conversion arguments: {arguments:?}"
                )));
            }
            verify_single_destination(destinations, is_string, "rune slice conversion")
        }
        hir::Builtin::StringToSliceRunes => {
            if !matches!(arguments, [argument] if is_string(argument)) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR string to rune slice conversion arguments: {arguments:?}"
                )));
            }
            verify_single_destination(
                destinations,
                is_rune_slice,
                "string to rune slice conversion",
            )
        }
        hir::Builtin::StringLen | hir::Builtin::StringRangeCount => {
            if !matches!(arguments, [value] if is_string(value)) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR string length arguments: {arguments:?}"
                )));
            }
            Ok(vec![Ty::Int(IntTy::Int)])
        }
        hir::Builtin::StringIndex => {
            verify_string_and_index(arguments, "index")?;
            Ok(vec![Ty::Uint(UintTy::Uint8)])
        }
        hir::Builtin::StringRange => {
            let [value, Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)] = arguments else {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR string slice arguments: {arguments:?}"
                )));
            };
            if !is_string(value) {
                return Err(Diagnostic::backend(format!(
                    "invalid MIR string slice arguments: {arguments:?}"
                )));
            }
            Ok(vec![value.clone()])
        }
        hir::Builtin::StringRangeIndexAt => {
            verify_string_and_index(arguments, "range index")?;
            Ok(vec![Ty::Int(IntTy::Int)])
        }
        hir::Builtin::StringRangeRuneAt => {
            verify_string_and_index(arguments, "range rune")?;
            Ok(vec![Ty::Int(IntTy::Int32)])
        }
        _ => Err(Diagnostic::backend(
            "non-string builtin reached string MIR verification",
        )),
    }
}

fn verify_string_and_index(arguments: &[Ty], context: &str) -> Result<(), Diagnostic> {
    if !matches!(arguments, [value, Ty::Int(IntTy::Int)] if is_string(value)) {
        return Err(Diagnostic::backend(format!(
            "invalid MIR string {context} arguments: {arguments:?}"
        )));
    }
    Ok(())
}

fn is_string(ty: &Ty) -> bool {
    ty.underlying() == &Ty::String
}

fn is_byte_slice(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8)
    )
}

fn is_rune_slice(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int32)
    )
}

fn verify_single_destination(
    destinations: &[Ty],
    predicate: fn(&Ty) -> bool,
    context: &str,
) -> Result<Vec<Ty>, Diagnostic> {
    let [destination] = destinations else {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} destinations: {destinations:?}"
        )));
    };
    if !predicate(destination) {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} destination: {destination:?}"
        )));
    }
    Ok(vec![destination.clone()])
}
