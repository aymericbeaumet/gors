//! Typed lowering for fixed-size Go arrays of bootstrap integer values.

use std::collections::{BTreeMap, BTreeSet};

use super::FunctionLowerer;
use super::expressions::{coerce_expr, expr_constant, parse_go_integer, validate_binary_operator};
use super::lower_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, IntTy, Ty};
use crate::token::Token;

const MAX_BOOTSTRAP_ARRAY_LENGTH: u64 = 1_048_576;

pub(super) fn lower_array_type(
    length: &ExprSyntax,
    element: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    let length = array_length(length, source)?;
    let element = lower_type(element, type_aliases, source)?;
    Ok(Ty::Array(length, Box::new(element)))
}

fn array_length(expression: &ExprSyntax, source: SourceRef) -> Result<u64, Diagnostic> {
    let spelling = match &expression.kind {
        ExprSyntaxKind::Literal {
            token: Token::INT,
            spelling,
        } => spelling.as_ref(),
        ExprSyntaxKind::Paren(expression) => return array_length(expression, source),
        _ => {
            return Err(Diagnostic::unsupported(
                "array lengths currently require an integer literal",
                source,
            ));
        }
    };
    let length = parse_go_integer(spelling)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            Diagnostic::semantic("array length must be a non-negative integer", source)
        })?;
    if length > MAX_BOOTSTRAP_ARRAY_LENGTH {
        return Err(Diagnostic::unsupported(
            format!(
                "array length {length} exceeds the current implementation limit of {MAX_BOOTSTRAP_ARRAY_LENGTH}"
            ),
            source,
        ));
    }
    Ok(length)
}

impl FunctionLowerer {
    pub(super) fn lower_array_literal(
        &mut self,
        literal_ty: Ty,
        elements: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Ty::Array(length, element_ty) = literal_ty.underlying() else {
            return Err(Diagnostic::backend(
                "array literal lowering received a non-array type",
            ));
        };
        if element_ty.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "array literals currently require int elements",
                source,
            ));
        }
        let length = usize::try_from(*length)
            .map_err(|_| Diagnostic::unsupported("array length does not fit this host", source))?;
        let mut values = vec![0; length];
        let mut initialized = BTreeSet::new();
        let mut next_index = 0usize;
        for element in elements {
            let (index, value) = match &element.kind {
                ExprSyntaxKind::KeyValue { key, value } => {
                    let (_, key) = super::eval_constant(key, &self.constants, source, 0)?;
                    let ConstValue::Int(key) = key else {
                        return Err(Diagnostic::semantic(
                            "array literal index must be an integer constant",
                            source,
                        ));
                    };
                    let index = key.parse::<usize>().map_err(|_| {
                        Diagnostic::semantic("array literal index is outside usize", source)
                    })?;
                    (index, value.as_ref())
                }
                _ => (next_index, element),
            };
            if index >= length {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is outside length {length}"),
                    source,
                ));
            }
            if !initialized.insert(index) {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is initialized more than once"),
                    source,
                ));
            }
            let value = self.lower_expr(value, Some(element_ty))?;
            let Some(ConstValue::Int(value)) = expr_constant(&value) else {
                return Err(Diagnostic::unsupported(
                    "dynamic array literal elements are not yet implemented",
                    source,
                ));
            };
            let value = value.parse::<i64>().map_err(|_| {
                Diagnostic::semantic("array literal element is outside Go int", source)
            })?;
            let slot = values.get_mut(index).ok_or_else(|| {
                Diagnostic::backend("validated array literal index is outside its storage")
            })?;
            *slot = value;
            next_index = index.saturating_add(1);
        }
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::ArrayLiteralI64(values),
            ty: literal_ty,
            category: hir::ValueCategory::Value,
            effects: hir::Effects::default(),
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_array_index(
        &mut self,
        array: hir::Expr,
        index: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Ty::Array(_, element) = array.ty.underlying() else {
            return Err(Diagnostic::backend(
                "array index lowering received a non-array value",
            ));
        };
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "array indexing currently supports int elements",
                source,
            ));
        }
        let result_ty = element.as_ref().clone();
        let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
        let effects = array.effects.union(index.effects).union(hir::Effects {
            may_panic: true,
            ..hir::Effects::default()
        });
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::ArrayIndexI64 {
                array: Box::new(array),
                index: Box::new(index),
            },
            ty: result_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_array_len(
        &mut self,
        array: hir::Expr,
        length: u64,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let effects = array.effects;
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::ArrayLen {
                array: Box::new(array),
                length,
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

    pub(super) fn lower_array_assignment(
        &mut self,
        container: hir::Expr,
        index: &ExprSyntax,
        token: Token,
        value: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let array = match &container.kind {
            hir::ExprKind::Local(local) => *local,
            _ => {
                return Err(Diagnostic::unsupported(
                    "array assignment currently requires a local array variable",
                    source,
                ));
            }
        };
        let Ty::Array(_, element) = container.ty.underlying() else {
            return Err(Diagnostic::backend(
                "array assignment lowering received a non-array value",
            ));
        };
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "array assignment currently supports int elements",
                source,
            ));
        }
        let element_ty = element.as_ref().clone();
        let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
        let value = self.lower_expr(value, Some(&element_ty))?;
        let op = super::statements::assignment_op(token, source)?;
        if op != hir::AssignOp::Set {
            validate_binary_operator(
                super::expressions::assignment_binary_op(op),
                &element_ty,
                source,
            )?;
        }
        Ok(hir::StmtKind::ArrayAssign {
            array,
            index,
            op,
            value,
        })
    }
}
