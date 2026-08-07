//! Typed lowering for Go's numeric built-in functions.

use super::FunctionLowerer;
use super::expressions::{
    coerce_expr, ensure_bootstrap_value_type, exact_common_operand_type, expr_constant,
    fold_constant_binary,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::ExprSyntax;
use crate::compiler::types::{ComplexTy, ConstValue, ExactNumber, FloatTy, Ty, UntypedTy};

impl FunctionLowerer {
    pub(super) fn lower_numeric_builtin_call(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic(
                format!("... is not valid in a call to {name}"),
                source,
            ));
        }
        match name {
            "min" | "max" => self.lower_min_max(name, arguments, node, source, expected),
            "complex" => self.lower_complex(arguments, node, source, expected),
            "real" | "imag" => {
                self.lower_complex_component(name, arguments, node, source, expected)
            }
            _ => Err(Diagnostic::backend(format!(
                "unknown numeric built-in {name}"
            ))),
        }
    }

    fn lower_min_max(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if arguments.is_empty() {
            return Err(Diagnostic::semantic(
                format!("call to {name} requires at least one argument"),
                source,
            ));
        }
        let mut values = arguments
            .iter()
            .map(|argument| self.lower_expr(argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        let first_value = values
            .first()
            .ok_or_else(|| Diagnostic::backend("missing min/max operand"))?;
        let exact_ty = values
            .iter()
            .skip(1)
            .try_fold(first_value.ty.clone(), |ty, value| {
                exact_common_operand_type(&ty, &value.ty).ok_or_else(|| {
                    Diagnostic::semantic(
                        format!("incompatible {name} operands {ty:?} and {:?}", value.ty),
                        source,
                    )
                })
            })?;
        let executable_ty = exact_ty.default_typed();
        ensure_bootstrap_value_type(&executable_ty, source)?;
        if !matches!(
            executable_ty.underlying(),
            Ty::Int(_) | Ty::Uint(_) | Ty::Float(_) | Ty::String
        ) {
            return Err(Diagnostic::semantic(
                format!("{name} requires ordered numeric arguments"),
                source,
            ));
        }
        let op = if name == "min" {
            hir::BinaryOp::Min
        } else {
            hir::BinaryOp::Max
        };
        if values.iter().all(|value| expr_constant(value).is_some()) {
            let first = values
                .first()
                .and_then(expr_constant)
                .ok_or_else(|| Diagnostic::backend("missing constant min/max operand"))?
                .clone();
            let folded = values.iter().skip(1).try_fold(first, |left, right| {
                fold_constant_binary(
                    op,
                    &left,
                    expr_constant(right)
                        .ok_or_else(|| Diagnostic::backend("missing constant min/max operand"))?,
                    source,
                )?
                .ok_or_else(|| Diagnostic::backend("numeric min/max did not constant-fold"))
            })?;
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::Constant(folded),
                ty: exact_ty,
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        if executable_ty == Ty::String {
            return Err(Diagnostic::unsupported(
                "runtime string min/max is not yet supported",
                source,
            ));
        }
        for value in &mut values {
            coerce_expr(value, &executable_ty, source)?;
        }
        let mut values = values.into_iter();
        let mut result = values
            .next()
            .ok_or_else(|| Diagnostic::backend("missing min/max operand"))?;
        if values.as_slice().is_empty() {
            let effects = result.effects;
            result = hir::Expr {
                node,
                kind: hir::ExprKind::Unary {
                    op: hir::UnaryOp::Positive,
                    operand: Box::new(result),
                },
                ty: executable_ty.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            };
        }
        for right in values {
            let effects = result.effects.union(right.effects);
            result = hir::Expr {
                node,
                kind: hir::ExprKind::Binary {
                    op,
                    left: Box::new(result),
                    right: Box::new(right),
                },
                ty: executable_ty.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            };
        }
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    fn lower_complex(
        &mut self,
        arguments: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [real_syntax, imag_syntax] = arguments else {
            return Err(Diagnostic::semantic(
                "call to complex requires exactly two arguments",
                source,
            ));
        };
        let mut real = self.lower_expr(real_syntax, None)?;
        let mut imag = self.lower_expr(imag_syntax, None)?;
        let component_ty = exact_common_operand_type(&real.ty, &imag.ty).ok_or_else(|| {
            Diagnostic::semantic(
                format!(
                    "incompatible complex components {:?} and {:?}",
                    real.ty, imag.ty
                ),
                source,
            )
        })?;
        let valid_component = matches!(
            component_ty.underlying(),
            Ty::Float(FloatTy::Float64)
                | Ty::Untyped(UntypedTy::Int | UntypedTy::Rune | UntypedTy::Float)
        );
        if !valid_component {
            return Err(Diagnostic::semantic(
                "complex requires floating-point arguments or numeric constants",
                source,
            ));
        }
        if let (Some(real), Some(imag)) = (constant_component(&real), constant_component(&imag)) {
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Complex { real, imag }),
                ty: if matches!(component_ty, Ty::Untyped(_)) {
                    Ty::Untyped(UntypedTy::Complex)
                } else {
                    Ty::Complex(ComplexTy::Complex128)
                },
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        let float = Ty::Float(FloatTy::Float64);
        coerce_expr(&mut real, &float, source)?;
        coerce_expr(&mut imag, &float, source)?;
        let effects = real.effects.union(imag.effects);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Binary {
                op: hir::BinaryOp::Complex,
                left: Box::new(real),
                right: Box::new(imag),
            },
            ty: Ty::Complex(ComplexTy::Complex128),
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    fn lower_complex_component(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [argument] = arguments else {
            return Err(Diagnostic::semantic(
                format!("call to {name} requires exactly one argument"),
                source,
            ));
        };
        let argument = self.lower_expr(argument, None)?;
        let Ty::Complex(complex_ty) = argument.ty.default_typed().underlying().clone() else {
            return Err(Diagnostic::semantic(
                format!("{name} requires a complex argument"),
                source,
            ));
        };
        let component_ty = complex_ty.component_type();
        let constant = match expr_constant(&argument) {
            Some(ConstValue::Complex { real, imag }) => Some(if name == "real" {
                real.clone()
            } else {
                imag.clone()
            }),
            Some(value @ (ConstValue::Int(_) | ConstValue::Float(_))) => {
                if name == "real" {
                    value.exact_number()
                } else {
                    Some(ExactNumber::zero())
                }
            }
            _ => None,
        };
        let mut result = if let Some(component) = constant {
            hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Float(component)),
                ty: if matches!(argument.ty, Ty::Untyped(_)) {
                    Ty::Untyped(UntypedTy::Float)
                } else {
                    Ty::Float(component_ty)
                },
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            }
        } else {
            let effects = argument.effects;
            hir::Expr {
                node,
                kind: hir::ExprKind::Unary {
                    op: if name == "real" {
                        hir::UnaryOp::Real
                    } else {
                        hir::UnaryOp::Imag
                    },
                    operand: Box::new(argument),
                },
                ty: Ty::Float(component_ty),
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

fn constant_component(expression: &hir::Expr) -> Option<ExactNumber> {
    expr_constant(expression)?.exact_number()
}
