//! Typed lowering for executable Go channel element representations.

use super::FunctionLowerer;
use super::expressions::coerce_expr;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ChannelDir, ConstValue, IntTy, Ty};
use crate::token::Token;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ChannelRepresentation {
    I64,
    GoString,
    GoChannelI64,
}

#[derive(Clone, Copy)]
pub(super) struct ChannelBuiltins {
    pub(super) nil: hir::Builtin,
    pub(super) make: hir::Builtin,
    pub(super) len: hir::Builtin,
    pub(super) cap: hir::Builtin,
    pub(super) send: hir::Builtin,
    pub(super) receive_value: hir::Builtin,
    pub(super) receive: hir::Builtin,
    pub(super) close: hir::Builtin,
    pub(super) is_nil: hir::Builtin,
    pub(super) try_send: hir::Builtin,
    pub(super) try_receive: hir::Builtin,
}

impl ChannelRepresentation {
    pub(super) const fn builtins(self) -> ChannelBuiltins {
        match self {
            Self::I64 => ChannelBuiltins {
                nil: hir::Builtin::ChannelI64Nil,
                make: hir::Builtin::ChannelI64Make,
                len: hir::Builtin::ChannelI64Len,
                cap: hir::Builtin::ChannelI64Cap,
                send: hir::Builtin::ChannelI64Send,
                receive_value: hir::Builtin::ChannelI64ReceiveValue,
                receive: hir::Builtin::ChannelI64Receive,
                close: hir::Builtin::ChannelI64Close,
                is_nil: hir::Builtin::ChannelI64IsNil,
                try_send: hir::Builtin::ChannelI64TrySend,
                try_receive: hir::Builtin::ChannelI64TryReceive,
            },
            Self::GoString => ChannelBuiltins {
                nil: hir::Builtin::ChannelGoStringNil,
                make: hir::Builtin::ChannelGoStringMake,
                len: hir::Builtin::ChannelGoStringLen,
                cap: hir::Builtin::ChannelGoStringCap,
                send: hir::Builtin::ChannelGoStringSend,
                receive_value: hir::Builtin::ChannelGoStringReceiveValue,
                receive: hir::Builtin::ChannelGoStringReceive,
                close: hir::Builtin::ChannelGoStringClose,
                is_nil: hir::Builtin::ChannelGoStringIsNil,
                try_send: hir::Builtin::ChannelGoStringTrySend,
                try_receive: hir::Builtin::ChannelGoStringTryReceive,
            },
            Self::GoChannelI64 => ChannelBuiltins {
                nil: hir::Builtin::ChannelGoChannelI64Nil,
                make: hir::Builtin::ChannelGoChannelI64Make,
                len: hir::Builtin::ChannelGoChannelI64Len,
                cap: hir::Builtin::ChannelGoChannelI64Cap,
                send: hir::Builtin::ChannelGoChannelI64Send,
                receive_value: hir::Builtin::ChannelGoChannelI64ReceiveValue,
                receive: hir::Builtin::ChannelGoChannelI64Receive,
                close: hir::Builtin::ChannelGoChannelI64Close,
                is_nil: hir::Builtin::ChannelGoChannelI64IsNil,
                try_send: hir::Builtin::ChannelGoChannelI64TrySend,
                try_receive: hir::Builtin::ChannelGoChannelI64TryReceive,
            },
        }
    }
}

fn channel_representation(element: &Ty) -> Option<ChannelRepresentation> {
    match element.underlying() {
        Ty::Int(IntTy::Int) => Some(ChannelRepresentation::I64),
        Ty::String => Some(ChannelRepresentation::GoString),
        Ty::Channel(_, nested) if nested.underlying() == &Ty::Int(IntTy::Int) => {
            Some(ChannelRepresentation::GoChannelI64)
        }
        _ => None,
    }
}

pub(super) fn channel_parts(ty: &Ty) -> Option<(ChannelDir, &Ty, ChannelRepresentation)> {
    let Ty::Channel(direction, element) = ty.underlying() else {
        return None;
    };
    let representation = channel_representation(element)?;
    Some((*direction, element.as_ref(), representation))
}

impl FunctionLowerer {
    pub(super) fn lower_channel_make(
        &mut self,
        declared: Ty,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic("make does not accept ...", source));
        }
        let Some((_, _, representation)) = channel_parts(&declared) else {
            return Err(Diagnostic::unsupported(
                "make does not yet support this channel element representation",
                source,
            ));
        };
        let capacity = match arguments {
            [_] => hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Int("0".into())),
                ty: Ty::Int(IntTy::Int),
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            },
            [_, capacity] => self.lower_expr(capacity, Some(&Ty::Int(IntTy::Int)))?,
            _ => {
                return Err(Diagnostic::semantic(
                    "make(channel[, capacity]) requires one or two arguments",
                    source,
                ));
            }
        };
        let effects = channel_effects(&[&capacity], false, true, false, true);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.builtins().make),
                args: vec![capacity],
            },
            ty: declared,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_channel_receive(
        &mut self,
        expression: &ExprSyntax,
        comma_ok: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let channel = self.lower_expr(expression, None)?;
        let Some((direction, element, representation)) = channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "receive requires an executable channel element representation",
                source,
            ));
        };
        if !direction.can_receive() {
            return Err(Diagnostic::semantic(
                "cannot receive from a send-only channel",
                source,
            ));
        }
        let element = element.clone();
        let ty = if comma_ok {
            Ty::Tuple(vec![element, Ty::Bool])
        } else {
            element
        };
        let effects = channel_effects(&[&channel], true, false, true, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(if comma_ok {
                    representation.builtins().receive
                } else {
                    representation.builtins().receive_value
                }),
                args: vec![channel],
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if !comma_ok && let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn try_lower_channel_comma_ok(
        &mut self,
        expression: &ExprSyntax,
    ) -> Option<Result<hir::Expr, Diagnostic>> {
        let ExprSyntaxKind::Unary {
            token: Token::ARROW,
            expression: channel,
        } = &expression.kind
        else {
            return None;
        };
        Some((|| {
            let node = self.alloc_node(expression.source)?;
            let source = SourceRef::node(node);
            self.lower_channel_receive(channel, true, node, source, None)
        })())
    }

    pub(super) fn lower_channel_send(
        &mut self,
        channel: &ExprSyntax,
        value: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let channel = self.lower_expr(channel, None)?;
        let Some((direction, element, representation)) = channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "send requires an executable channel element representation",
                source,
            ));
        };
        if !direction.can_send() {
            return Err(Diagnostic::semantic(
                "cannot send to a receive-only channel",
                source,
            ));
        }
        let element = element.clone();
        let value = self.lower_expr(value, Some(&element))?;
        let effects = channel_effects(&[&channel, &value], true, false, true, true);
        Ok(hir::StmtKind::Expr(hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.builtins().send),
                args: vec![channel, value],
            },
            ty: Ty::Unit,
            category: hir::ValueCategory::Value,
            effects,
            source,
        }))
    }

    pub(super) fn lower_channel_try_send(
        &mut self,
        channel: &ExprSyntax,
        value: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let channel = self.lower_expr(channel, None)?;
        let Some((direction, element, representation)) = channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "select send requires an executable channel element representation",
                source,
            ));
        };
        if !direction.can_send() {
            return Err(Diagnostic::semantic(
                "cannot send to a receive-only channel",
                source,
            ));
        }
        let value = self.lower_expr(value, Some(element))?;
        let effects = channel_effects(&[&channel, &value], true, false, false, true);
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.builtins().try_send),
                args: vec![channel, value],
            },
            ty: Ty::Bool,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn lower_channel_try_receive(
        &mut self,
        expression: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let channel = self.lower_expr(expression, None)?;
        let Some((direction, element, representation)) = channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "select receive requires an executable channel element representation",
                source,
            ));
        };
        if !direction.can_receive() {
            return Err(Diagnostic::semantic(
                "cannot receive from a send-only channel",
                source,
            ));
        }
        let ty = Ty::Tuple(vec![element.clone(), Ty::Int(IntTy::Int)]);
        let effects = channel_effects(&[&channel], true, false, false, false);
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.builtins().try_receive),
                args: vec![channel],
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn lower_channel_close_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [channel] = arguments else {
            return Err(Diagnostic::semantic(
                "close requires exactly one channel",
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic("close does not accept ...", source));
        }
        let channel = self.lower_expr(channel, None)?;
        let Some((direction, _, representation)) = channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "close requires an executable channel element representation",
                source,
            ));
        };
        if !direction.can_send() {
            return Err(Diagnostic::semantic(
                "cannot close a receive-only channel",
                source,
            ));
        }
        let effects = channel_effects(&[&channel], true, false, false, true);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.builtins().close),
                args: vec![channel],
            },
            ty: Ty::Unit,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

pub(super) fn channel_effects(
    arguments: &[&hir::Expr],
    writes: bool,
    allocates: bool,
    blocks: bool,
    panics: bool,
) -> hir::Effects {
    arguments.iter().fold(
        hir::Effects {
            may_call: true,
            may_allocate: allocates,
            may_block: blocks,
            may_panic: panics,
            may_write: writes,
            ..hir::Effects::default()
        },
        |effects, argument| effects.union(argument.effects),
    )
}
