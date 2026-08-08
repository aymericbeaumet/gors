//! MIR call verification for executable channel representations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{ChannelDir, IntTy, Ty};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChannelRepresentation {
    I64,
    GoString,
    GoChannelI64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ChannelOperation {
    Nil,
    Make,
    Len,
    Cap,
    Send,
    ReceiveValue,
    Receive,
    Close,
    IsNil,
    TrySend,
    TryReceive,
}

pub(super) fn is_channel_builtin(builtin: hir::Builtin) -> bool {
    channel_builtin_parts(builtin).is_some()
}

pub(super) fn verify_channel_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    let Some((representation, operation)) = channel_builtin_parts(builtin) else {
        return Err(Diagnostic::backend(
            "non-channel builtin reached channel MIR verification",
        ));
    };
    match operation {
        ChannelOperation::Nil => {
            let [result] = destinations else {
                return Err(shape_error("nil channel", arguments, destinations));
            };
            verify_channel(result, representation, "nil channel result")?;
            if !arguments.is_empty() {
                return Err(shape_error("nil channel", arguments, destinations));
            }
            Ok(vec![result.clone()])
        }
        ChannelOperation::Make => {
            let ([capacity], [result]) = (arguments, destinations) else {
                return Err(shape_error("channel make", arguments, destinations));
            };
            verify_int(capacity, "channel capacity")?;
            verify_channel(result, representation, "channel make result")?;
            Ok(vec![result.clone()])
        }
        ChannelOperation::Len | ChannelOperation::Cap => {
            let [channel] = arguments else {
                return Err(shape_error("channel size", arguments, destinations));
            };
            verify_channel(channel, representation, "channel size")?;
            Ok(vec![Ty::Int(IntTy::Int)])
        }
        ChannelOperation::Send => {
            let [channel, value] = arguments else {
                return Err(shape_error("channel send", arguments, destinations));
            };
            let (direction, element) = verify_channel(channel, representation, "channel send")?;
            if !direction.can_send() {
                return Err(Diagnostic::backend("receive-only channel reached MIR send"));
            }
            if value != element || !destinations.is_empty() {
                return Err(shape_error("channel send", arguments, destinations));
            }
            Ok(Vec::new())
        }
        ChannelOperation::ReceiveValue | ChannelOperation::Receive => {
            let [channel] = arguments else {
                return Err(shape_error("channel receive", arguments, destinations));
            };
            let (direction, element) = verify_channel(channel, representation, "channel receive")?;
            if !direction.can_receive() {
                return Err(Diagnostic::backend("send-only channel reached MIR receive"));
            }
            if operation == ChannelOperation::Receive {
                Ok(vec![element.clone(), Ty::Bool])
            } else {
                Ok(vec![element.clone()])
            }
        }
        ChannelOperation::Close => {
            let [channel] = arguments else {
                return Err(shape_error("channel close", arguments, destinations));
            };
            let (direction, _) = verify_channel(channel, representation, "channel close")?;
            if !direction.can_send() || !destinations.is_empty() {
                return Err(shape_error("channel close", arguments, destinations));
            }
            Ok(Vec::new())
        }
        ChannelOperation::IsNil => {
            let [channel] = arguments else {
                return Err(shape_error(
                    "channel nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_channel(channel, representation, "channel nil comparison")?;
            Ok(vec![Ty::Bool])
        }
        ChannelOperation::TrySend => {
            let [channel, value] = arguments else {
                return Err(shape_error("channel select send", arguments, destinations));
            };
            let (direction, element) =
                verify_channel(channel, representation, "channel select send")?;
            if !direction.can_send() || value != element {
                return Err(shape_error("channel select send", arguments, destinations));
            }
            Ok(vec![Ty::Bool])
        }
        ChannelOperation::TryReceive => {
            let [channel] = arguments else {
                return Err(shape_error(
                    "channel select receive",
                    arguments,
                    destinations,
                ));
            };
            let (direction, element) =
                verify_channel(channel, representation, "channel select receive")?;
            if !direction.can_receive() {
                return Err(Diagnostic::backend(
                    "send-only channel reached MIR select receive",
                ));
            }
            Ok(vec![element.clone(), Ty::Int(IntTy::Int)])
        }
    }
}

fn channel_builtin_parts(
    builtin: hir::Builtin,
) -> Option<(ChannelRepresentation, ChannelOperation)> {
    let parts = match builtin {
        hir::Builtin::ChannelI64Nil => (ChannelRepresentation::I64, ChannelOperation::Nil),
        hir::Builtin::ChannelI64Make => (ChannelRepresentation::I64, ChannelOperation::Make),
        hir::Builtin::ChannelI64Len => (ChannelRepresentation::I64, ChannelOperation::Len),
        hir::Builtin::ChannelI64Cap => (ChannelRepresentation::I64, ChannelOperation::Cap),
        hir::Builtin::ChannelI64Send => (ChannelRepresentation::I64, ChannelOperation::Send),
        hir::Builtin::ChannelI64ReceiveValue => {
            (ChannelRepresentation::I64, ChannelOperation::ReceiveValue)
        }
        hir::Builtin::ChannelI64Receive => (ChannelRepresentation::I64, ChannelOperation::Receive),
        hir::Builtin::ChannelI64Close => (ChannelRepresentation::I64, ChannelOperation::Close),
        hir::Builtin::ChannelI64IsNil => (ChannelRepresentation::I64, ChannelOperation::IsNil),
        hir::Builtin::ChannelI64TrySend => (ChannelRepresentation::I64, ChannelOperation::TrySend),
        hir::Builtin::ChannelI64TryReceive => {
            (ChannelRepresentation::I64, ChannelOperation::TryReceive)
        }
        hir::Builtin::ChannelGoStringNil => {
            (ChannelRepresentation::GoString, ChannelOperation::Nil)
        }
        hir::Builtin::ChannelGoStringMake => {
            (ChannelRepresentation::GoString, ChannelOperation::Make)
        }
        hir::Builtin::ChannelGoStringLen => {
            (ChannelRepresentation::GoString, ChannelOperation::Len)
        }
        hir::Builtin::ChannelGoStringCap => {
            (ChannelRepresentation::GoString, ChannelOperation::Cap)
        }
        hir::Builtin::ChannelGoStringSend => {
            (ChannelRepresentation::GoString, ChannelOperation::Send)
        }
        hir::Builtin::ChannelGoStringReceiveValue => (
            ChannelRepresentation::GoString,
            ChannelOperation::ReceiveValue,
        ),
        hir::Builtin::ChannelGoStringReceive => {
            (ChannelRepresentation::GoString, ChannelOperation::Receive)
        }
        hir::Builtin::ChannelGoStringClose => {
            (ChannelRepresentation::GoString, ChannelOperation::Close)
        }
        hir::Builtin::ChannelGoStringIsNil => {
            (ChannelRepresentation::GoString, ChannelOperation::IsNil)
        }
        hir::Builtin::ChannelGoStringTrySend => {
            (ChannelRepresentation::GoString, ChannelOperation::TrySend)
        }
        hir::Builtin::ChannelGoStringTryReceive => (
            ChannelRepresentation::GoString,
            ChannelOperation::TryReceive,
        ),
        hir::Builtin::ChannelGoChannelI64Nil => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Nil)
        }
        hir::Builtin::ChannelGoChannelI64Make => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Make)
        }
        hir::Builtin::ChannelGoChannelI64Len => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Len)
        }
        hir::Builtin::ChannelGoChannelI64Cap => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Cap)
        }
        hir::Builtin::ChannelGoChannelI64Send => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Send)
        }
        hir::Builtin::ChannelGoChannelI64ReceiveValue => (
            ChannelRepresentation::GoChannelI64,
            ChannelOperation::ReceiveValue,
        ),
        hir::Builtin::ChannelGoChannelI64Receive => (
            ChannelRepresentation::GoChannelI64,
            ChannelOperation::Receive,
        ),
        hir::Builtin::ChannelGoChannelI64Close => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::Close)
        }
        hir::Builtin::ChannelGoChannelI64IsNil => {
            (ChannelRepresentation::GoChannelI64, ChannelOperation::IsNil)
        }
        hir::Builtin::ChannelGoChannelI64TrySend => (
            ChannelRepresentation::GoChannelI64,
            ChannelOperation::TrySend,
        ),
        hir::Builtin::ChannelGoChannelI64TryReceive => (
            ChannelRepresentation::GoChannelI64,
            ChannelOperation::TryReceive,
        ),
        _ => return None,
    };
    Some(parts)
}

fn verify_channel<'a>(
    ty: &'a Ty,
    representation: ChannelRepresentation,
    context: &str,
) -> Result<(ChannelDir, &'a Ty), Diagnostic> {
    let Ty::Channel(direction, element) = ty.underlying() else {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )));
    };
    if !element_matches_representation(element, representation) {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} channel element type: {element:?}"
        )));
    }
    Ok((*direction, element))
}

fn element_matches_representation(element: &Ty, representation: ChannelRepresentation) -> bool {
    match representation {
        ChannelRepresentation::I64 => element.underlying() == &Ty::Int(IntTy::Int),
        ChannelRepresentation::GoString => element.underlying() == &Ty::String,
        ChannelRepresentation::GoChannelI64 => matches!(
            element.underlying(),
            Ty::Channel(_, nested) if nested.underlying() == &Ty::Int(IntTy::Int)
        ),
    }
}

fn verify_int(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if ty.underlying() == &Ty::Int(IntTy::Int) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "invalid MIR {context} integer type: {ty:?}"
        )))
    }
}

fn shape_error(context: &str, arguments: &[Ty], destinations: &[Ty]) -> Diagnostic {
    Diagnostic::backend(format!(
        "invalid MIR {context} shape: {arguments:?} -> {destinations:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(element: Ty) -> Ty {
        Ty::Channel(ChannelDir::SendReceive, Box::new(element))
    }

    #[test]
    fn string_channel_builtins_require_the_exact_string_family() {
        let string_channel = channel(Ty::String);
        assert!(
            verify_channel_call(
                hir::Builtin::ChannelGoStringSend,
                &[string_channel.clone(), Ty::String],
                &[],
            )
            .is_ok()
        );
        assert!(
            verify_channel_call(
                hir::Builtin::ChannelGoStringSend,
                &[string_channel, Ty::Int(IntTy::Int)],
                &[],
            )
            .is_err()
        );
    }

    #[test]
    fn nested_channel_builtins_reject_a_string_channel_outer_value() {
        let inner = channel(Ty::Int(IntTy::Int));
        let nested = channel(inner.clone());
        let expected = vec![inner.clone(), Ty::Bool];
        let result = verify_channel_call(
            hir::Builtin::ChannelGoChannelI64Receive,
            &[nested],
            &[inner, Ty::Bool],
        );
        assert_eq!(
            result.as_ref().ok(),
            Some(&expected),
            "nested channel receive must verify: {result:?}"
        );
        assert!(
            verify_channel_call(
                hir::Builtin::ChannelGoChannelI64ReceiveValue,
                &[channel(Ty::String)],
                &[Ty::String],
            )
            .is_err()
        );
    }
}
