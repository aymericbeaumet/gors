//! MIR call verification for bootstrap integer channel operations.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::{ChannelDir, IntTy, Ty};

pub(super) fn is_channel_builtin(builtin: hir::Builtin) -> bool {
    matches!(
        builtin,
        hir::Builtin::ChannelI64Nil
            | hir::Builtin::ChannelI64Make
            | hir::Builtin::ChannelI64Len
            | hir::Builtin::ChannelI64Cap
            | hir::Builtin::ChannelI64Send
            | hir::Builtin::ChannelI64ReceiveValue
            | hir::Builtin::ChannelI64Receive
            | hir::Builtin::ChannelI64Close
            | hir::Builtin::ChannelI64IsNil
    )
}

pub(super) fn verify_channel_call(
    builtin: hir::Builtin,
    arguments: &[Ty],
    destinations: &[Ty],
) -> Result<Vec<Ty>, Diagnostic> {
    match builtin {
        hir::Builtin::ChannelI64Nil => {
            let [result] = destinations else {
                return Err(shape_error("nil channel", arguments, destinations));
            };
            verify_int_channel(result, "nil channel result")?;
            if !arguments.is_empty() {
                return Err(shape_error("nil channel", arguments, destinations));
            }
            Ok(vec![result.clone()])
        }
        hir::Builtin::ChannelI64Make => {
            let ([capacity], [result]) = (arguments, destinations) else {
                return Err(shape_error("channel make", arguments, destinations));
            };
            verify_int(capacity, "channel capacity")?;
            verify_int_channel(result, "channel make result")?;
            Ok(vec![result.clone()])
        }
        hir::Builtin::ChannelI64Len | hir::Builtin::ChannelI64Cap => {
            let [channel] = arguments else {
                return Err(shape_error("channel size", arguments, destinations));
            };
            verify_int_channel(channel, "channel size")?;
            Ok(vec![Ty::Int(IntTy::Int)])
        }
        hir::Builtin::ChannelI64Send => {
            let [channel, value] = arguments else {
                return Err(shape_error("channel send", arguments, destinations));
            };
            let (direction, element) = verify_int_channel(channel, "channel send")?;
            if !direction.can_send() {
                return Err(Diagnostic::backend("receive-only channel reached MIR send"));
            }
            if value != element || !destinations.is_empty() {
                return Err(shape_error("channel send", arguments, destinations));
            }
            Ok(Vec::new())
        }
        hir::Builtin::ChannelI64ReceiveValue | hir::Builtin::ChannelI64Receive => {
            let [channel] = arguments else {
                return Err(shape_error("channel receive", arguments, destinations));
            };
            let (direction, element) = verify_int_channel(channel, "channel receive")?;
            if !direction.can_receive() {
                return Err(Diagnostic::backend("send-only channel reached MIR receive"));
            }
            if builtin == hir::Builtin::ChannelI64Receive {
                Ok(vec![element.clone(), Ty::Bool])
            } else {
                Ok(vec![element.clone()])
            }
        }
        hir::Builtin::ChannelI64Close => {
            let [channel] = arguments else {
                return Err(shape_error("channel close", arguments, destinations));
            };
            let (direction, _) = verify_int_channel(channel, "channel close")?;
            if !direction.can_send() || !destinations.is_empty() {
                return Err(shape_error("channel close", arguments, destinations));
            }
            Ok(Vec::new())
        }
        hir::Builtin::ChannelI64IsNil => {
            let [channel] = arguments else {
                return Err(shape_error(
                    "channel nil comparison",
                    arguments,
                    destinations,
                ));
            };
            verify_int_channel(channel, "channel nil comparison")?;
            Ok(vec![Ty::Bool])
        }
        _ => Err(Diagnostic::backend(
            "non-channel builtin reached channel MIR verification",
        )),
    }
}

fn verify_int_channel<'a>(ty: &'a Ty, context: &str) -> Result<(ChannelDir, &'a Ty), Diagnostic> {
    let Ty::Channel(direction, element) = ty.underlying() else {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} type: {ty:?}"
        )));
    };
    verify_int(element, context)?;
    Ok((*direction, element))
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
