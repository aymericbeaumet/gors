//! Composite-literal dispatch and executable slice literal lowering.

use super::FunctionLowerer;
use super::expressions::expr_constant;
use super::lower_type;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, SyntaxSource};
use crate::compiler::types::{ConstValue, IntTy, Ty, UintTy};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_composite_literal(
        &mut self,
        ty: Option<&ExprSyntax>,
        elements: &[ExprSyntax],
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Some(ty) = ty else {
            return Err(Diagnostic::unsupported(
                "elided composite literal types require an enclosing composite type",
                source,
            ));
        };
        let literal_ty = match &ty.kind {
            crate::compiler::syntax::ExprSyntaxKind::ArrayType {
                length: Some(length),
                element,
            } if matches!(
                length.kind,
                crate::compiler::syntax::ExprSyntaxKind::Unsupported("ellipsis")
            ) =>
            {
                Ty::Array(
                    self.infer_array_literal_length(elements, source)?,
                    Box::new(lower_type(element, &self.type_aliases, source)?),
                )
            }
            _ => lower_type(ty, &self.type_aliases, source)?,
        };
        if matches!(literal_ty.underlying(), Ty::Struct(_)) {
            return self.lower_struct_literal(literal_ty, elements, node, syntax_source, source);
        }
        if matches!(literal_ty.underlying(), Ty::Map(_, _)) {
            return self.lower_map_literal(literal_ty, elements, node, source);
        }
        if matches!(literal_ty.underlying(), Ty::Array(_, _)) {
            return self.lower_array_literal(
                literal_ty,
                elements,
                node,
                syntax_source,
                source,
                expected,
            );
        }
        let Ty::Slice(element_ty) = literal_ty.underlying() else {
            return Err(Diagnostic::unsupported(
                "this composite literal type is not yet implemented",
                source,
            ));
        };
        let integer_elements = element_ty.underlying() == &Ty::Int(IntTy::Int);
        let byte_elements = element_ty.underlying() == &Ty::Uint(UintTy::Uint8);
        let boolean_elements = element_ty.underlying() == &Ty::Bool;
        if !integer_elements && !byte_elements && !boolean_elements {
            return Err(Diagnostic::unsupported(
                "slice literals currently require bool, int, or byte elements",
                source,
            ));
        }
        if boolean_elements {
            let values = elements
                .iter()
                .map(|element| {
                    let element = self.lower_expr(element, Some(element_ty))?;
                    let Some(ConstValue::Bool(value)) = expr_constant(&element) else {
                        return Err(Diagnostic::unsupported(
                            "dynamic slice literal elements are not yet implemented",
                            source,
                        ));
                    };
                    Ok(*value)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::SliceLiteralBool(values),
                ty: literal_ty,
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
                source,
            });
        }
        let mut values = Vec::with_capacity(elements.len());
        let mut lowered_elements = Vec::with_capacity(elements.len());
        let mut dynamic = false;
        for element in elements {
            let element = self.lower_expr(element, Some(element_ty))?;
            if let Some(ConstValue::Int(value)) = expr_constant(&element) {
                values.push(value.parse::<i64>().map_err(|_| {
                    Diagnostic::semantic("slice literal element is outside Go int", source)
                })?);
            } else if byte_elements {
                return Err(Diagnostic::unsupported(
                    "dynamic byte slice literal elements are not yet implemented",
                    source,
                ));
            } else {
                dynamic = true;
            }
            lowered_elements.push(element);
        }
        let effects = lowered_elements
            .iter()
            .fold(hir::Effects::default(), |effects, element| {
                effects.union(element.effects)
            })
            .union(hir::Effects {
                may_call: dynamic,
                may_allocate: true,
                may_write: dynamic,
                may_panic: dynamic,
                ..hir::Effects::default()
            });
        let kind = if dynamic {
            hir::ExprKind::DynamicSliceLiteralI64(lowered_elements)
        } else if byte_elements {
            hir::ExprKind::SliceLiteralU8(
                values
                    .into_iter()
                    .map(|value| {
                        u8::try_from(value).map_err(|_| {
                            Diagnostic::semantic(
                                "byte slice literal element is outside byte range",
                                source,
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        } else {
            hir::ExprKind::SliceLiteralI64(values)
        };
        Ok(hir::Expr {
            node,
            kind,
            ty: literal_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }
}
