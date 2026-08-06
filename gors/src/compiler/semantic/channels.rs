//! Typed lowering for Go channels carrying bootstrap integer values.

use super::FunctionLowerer;
use super::expressions::coerce_expr;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ChannelDir, ConstValue, IntTy, Ty};
use crate::token::Token;

pub(super) fn int_channel_parts(ty: &Ty) -> Option<(ChannelDir, &Ty)> {
    let Ty::Channel(direction, element) = ty.underlying() else {
        return None;
    };
    (element.underlying() == &Ty::Int(IntTy::Int)).then_some((*direction, element.as_ref()))
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
        let Some(_) = int_channel_parts(&declared) else {
            return Err(Diagnostic::unsupported(
                "make currently supports channels carrying int values",
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
                    "make(chan int[, capacity]) requires one or two arguments",
                    source,
                ));
            }
        };
        let effects = channel_effects(&[&capacity], false, true, false, true);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Make),
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
        let Some((direction, element)) = int_channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "receive requires a channel carrying int values",
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
                    hir::Builtin::ChannelI64Receive
                } else {
                    hir::Builtin::ChannelI64ReceiveValue
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
        let Some((direction, element)) = int_channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "send requires a channel carrying int values",
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
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Send),
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
        let Some((direction, element)) = int_channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "select send requires a channel carrying int values",
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
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64TrySend),
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
        let Some((direction, element)) = int_channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "select receive requires a channel carrying int values",
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
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64TryReceive),
                args: vec![channel],
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn lower_channel_cap_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [value] = arguments else {
            return Err(Diagnostic::semantic(
                "cap requires exactly one argument",
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic("cap does not accept ...", source));
        }
        let value = self.lower_expr(value, None)?;
        let builtin = if int_channel_parts(&value.ty).is_some() {
            hir::Builtin::ChannelI64Cap
        } else if matches!(
            value.ty.underlying(),
            Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int)
        ) {
            hir::Builtin::SliceI64Cap
        } else {
            return Err(Diagnostic::unsupported(
                format!("cap is not yet implemented for {:?}", value.ty),
                source,
            ));
        };
        self.lower_channel_size(value, builtin, node, source, expected)
    }

    pub(super) fn lower_channel_len(
        &mut self,
        channel: hir::Expr,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        self.lower_channel_size(channel, hir::Builtin::ChannelI64Len, node, source, expected)
    }

    fn lower_channel_size(
        &mut self,
        channel: hir::Expr,
        builtin: hir::Builtin,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let effects = channel_effects(&[&channel], false, false, false, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args: vec![channel],
            },
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
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
        let Some((direction, _)) = int_channel_parts(&channel.ty) else {
            return Err(Diagnostic::semantic(
                "close requires a channel carrying int values",
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
                callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Close),
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
