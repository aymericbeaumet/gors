//! Composite-literal dispatch and executable slice literal lowering.

use super::FunctionLowerer;
use super::expressions::expr_constant;
use super::pointers::pointer_effects;
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
        let literal_ty = match ty {
            Some(ty) => match &ty.kind {
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
                        Box::new(self.lower_semantic_type(element, source)?),
                    )
                }
                _ => self.lower_semantic_type(ty, source)?,
            },
            None => expected.cloned().ok_or_else(|| {
                Diagnostic::semantic(
                    "elided composite literal requires an enclosing composite type",
                    source,
                )
            })?,
        };
        if let Ty::Pointer(element) = literal_ty.underlying()
            && element.bootstrap_i64_struct_fields().is_some()
        {
            let value_node = self.alloc_node(syntax_source)?;
            let value = self.lower_struct_literal(
                element.as_ref().clone(),
                elements,
                value_node,
                syntax_source,
                SourceRef::node(value_node),
            )?;
            let effects = pointer_effects(&[&value], true, true, false);
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::AddressOfValue(Box::new(value)),
                ty: literal_ty,
                category: hir::ValueCategory::Value,
                effects,
                source,
            });
        }
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
        let integer_elements =
            matches!(element_ty.underlying(), Ty::Int(IntTy::Int | IntTy::Int32));
        let byte_elements = element_ty.underlying() == &Ty::Uint(UintTy::Uint8);
        let boolean_elements = element_ty.underlying() == &Ty::Bool;
        let aggregate_elements = element_ty.uses_interface_aggregate_representation();
        if !integer_elements && !byte_elements && !boolean_elements && !aggregate_elements {
            return Err(Diagnostic::unsupported(
                "slice literal element type has no executable representation",
                source,
            ));
        }
        if aggregate_elements {
            let lowered_elements = elements
                .iter()
                .map(|element| self.lower_expr(element, Some(element_ty)))
                .collect::<Result<Vec<_>, _>>()?;
            let type_identity = element_ty.dynamic_type_identity().ok_or_else(|| {
                Diagnostic::backend("aggregate slice element omitted its dynamic type identity")
            })?;
            let effects = lowered_elements
                .iter()
                .fold(hir::Effects::default(), |effects, element| {
                    effects.union(element.effects)
                })
                .union(hir::Effects {
                    may_call: true,
                    may_allocate: true,
                    may_write: true,
                    may_panic: true,
                    ..hir::Effects::default()
                });
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::AggregateSliceLiteral {
                    elements: lowered_elements,
                    type_identity,
                },
                ty: literal_ty,
                category: hir::ValueCategory::Value,
                effects,
                source,
            });
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
