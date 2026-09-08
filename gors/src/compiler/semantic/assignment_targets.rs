//! Typed, source-provenanced assignment-target construction.

use super::FunctionLowerer;
use super::arrays::is_executable_array_element;
use super::maps::{i64_go_string_map_ty, string_i64_map_ty};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty, UintTy};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn lower_assignment_targets(
        &mut self,
        expressions: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<Vec<hir::AssignTarget>, Diagnostic> {
        expressions
            .iter()
            .map(|expression| self.lower_assignment_target(expression, source))
            .collect()
    }

    pub(super) fn lower_assignment_target(
        &mut self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::AssignTarget, Diagnostic> {
        match &expression.kind {
            ExprSyntaxKind::Paren(inner) => {
                let mut target = self.lower_assignment_target(inner, source)?;
                target.source = SourceRef::node(self.alloc_node(expression.source)?);
                Ok(target)
            }
            ExprSyntaxKind::Ident(_) => {
                let place = self.lower_place(expression, source)?;
                let target_source = SourceRef::node(self.alloc_node(expression.source)?);
                match place {
                    hir::Place::Local(local) => Ok(hir::AssignTarget {
                        kind: hir::AssignTargetKind::Local(local),
                        ty: Some(self.place_ty(place)?.clone()),
                        source: target_source,
                    }),
                    hir::Place::Discard => Ok(hir::AssignTarget {
                        kind: hir::AssignTargetKind::Discard,
                        ty: None,
                        source: target_source,
                    }),
                }
            }
            ExprSyntaxKind::Index { base, index } => {
                self.lower_index_assignment_target(expression, base, index, source)
            }
            ExprSyntaxKind::Unary {
                token: Token::MUL,
                expression: pointer,
            } => {
                let pointer = self.lower_expr(pointer, None)?;
                let Ty::Pointer(element) = pointer.ty.underlying() else {
                    return Err(Diagnostic::semantic(
                        "indirect assignment requires a pointer",
                        source,
                    ));
                };
                if element.underlying() != &Ty::Int(IntTy::Int) {
                    return Err(Diagnostic::unsupported(
                        "indirect assignment currently supports pointers to Go int values",
                        source,
                    ));
                }
                let element_ty = element.as_ref().clone();
                Ok(hir::AssignTarget {
                    kind: hir::AssignTargetKind::Pointer {
                        pointer,
                        set: hir::Builtin::PointerI64Set,
                    },
                    ty: Some(element_ty),
                    source: SourceRef::node(self.alloc_node(expression.source)?),
                })
            }
            ExprSyntaxKind::Selector { base, member } => {
                let node = self.alloc_node(expression.source)?;
                let target_source = SourceRef::node(node);
                let target = self.lower_selector(base, member, node, target_source)?;
                let target_ty = target.ty.clone();
                if let Some((structure, fields)) = local_struct_field_path(&target) {
                    return Ok(hir::AssignTarget {
                        kind: hir::AssignTargetKind::StructFieldPath { structure, fields },
                        ty: Some(target_ty),
                        source: target_source,
                    });
                }
                if let Some((pointer, field)) = pointer_struct_field(&target)? {
                    return Ok(hir::AssignTarget {
                        kind: hir::AssignTargetKind::PointerStructField {
                            pointer,
                            field,
                            set: hir::Builtin::PointerStructI64Set,
                        },
                        ty: Some(target_ty),
                        source: target_source,
                    });
                }
                Err(Diagnostic::unsupported(
                    "assignment through this nested selector path is not yet represented",
                    source,
                ))
            }
            _ => Err(Diagnostic::unsupported(
                "this assignment target is not yet represented",
                source,
            )),
        }
    }

    fn lower_index_assignment_target(
        &mut self,
        expression: &ExprSyntax,
        base: &ExprSyntax,
        index: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::AssignTarget, Diagnostic> {
        let container = self.lower_expr(base, None)?;
        let target_source = SourceRef::node(self.alloc_node(expression.source)?);
        match container.ty.underlying() {
            Ty::Array(_, element) if is_executable_array_element(element) => {
                let hir::ExprKind::Local(array) = container.kind else {
                    return Err(Diagnostic::unsupported(
                        "array element assignment currently requires a local array variable",
                        source,
                    ));
                };
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                Ok(hir::AssignTarget {
                    kind: hir::AssignTargetKind::ArrayIndex { array, index },
                    ty: Some(element.as_ref().clone()),
                    source: target_source,
                })
            }
            Ty::Slice(element)
                if matches!(
                    element.underlying(),
                    Ty::Int(_) | Ty::Uint(_) | Ty::Bool | Ty::String
                ) =>
            {
                let set = if element.underlying() == &Ty::Bool {
                    hir::Builtin::SliceBoolSet
                } else if element.underlying() == &Ty::Uint(UintTy::Uint8) {
                    hir::Builtin::SliceU8Set
                } else if element.underlying() == &Ty::String {
                    hir::Builtin::SliceGoStringSet
                } else {
                    hir::Builtin::SliceI64Set
                };
                let ty = element.as_ref().clone();
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                Ok(hir::AssignTarget {
                    kind: hir::AssignTargetKind::SliceIndex {
                        slice: container,
                        index,
                        set,
                    },
                    ty: Some(ty),
                    source: target_source,
                })
            }
            Ty::Map(_, _) if container.ty.underlying() == string_i64_map_ty().underlying() => {
                let key = self.lower_expr(index, Some(&Ty::String))?;
                Ok(hir::AssignTarget {
                    kind: hir::AssignTargetKind::MapIndex {
                        map: container,
                        key,
                    },
                    ty: Some(Ty::Int(IntTy::Int)),
                    source: target_source,
                })
            }
            Ty::Map(_, _) if container.ty.underlying() == i64_go_string_map_ty().underlying() => {
                let key = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                Ok(hir::AssignTarget {
                    kind: hir::AssignTargetKind::MapIndex {
                        map: container,
                        key,
                    },
                    ty: Some(Ty::String),
                    source: target_source,
                })
            }
            _ => Err(Diagnostic::semantic(
                "indexed assignment requires an executable array, slice, or map representation",
                source,
            )),
        }
    }
}

fn local_struct_field_path(
    expression: &hir::Expr,
) -> Option<(crate::compiler::ids::LocalId, Vec<u32>)> {
    fn walk(
        expression: &hir::Expr,
        fields: &mut Vec<u32>,
    ) -> Option<crate::compiler::ids::LocalId> {
        match &expression.kind {
            hir::ExprKind::Local(local) => Some(*local),
            hir::ExprKind::StructField { structure, field } => {
                let local = walk(structure, fields)?;
                fields.push(*field);
                Some(local)
            }
            _ => None,
        }
    }

    let mut fields = Vec::new();
    let structure = walk(expression, &mut fields)?;
    (!fields.is_empty()).then_some((structure, fields))
}

fn pointer_struct_field(expression: &hir::Expr) -> Result<Option<(hir::Expr, u32)>, Diagnostic> {
    let hir::ExprKind::Call {
        callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Get),
        args,
    } = &expression.kind
    else {
        return Ok(None);
    };
    let [pointer, index] = args.as_slice() else {
        return Err(Diagnostic::backend(
            "pointer field selector has invalid operand arity",
        ));
    };
    let hir::ExprKind::Constant(crate::compiler::types::ConstValue::Int(index)) = &index.kind
    else {
        return Err(Diagnostic::backend(
            "pointer field selector has a dynamic field index",
        ));
    };
    let field = index
        .parse::<u32>()
        .map_err(|_| Diagnostic::backend("pointer field selector index does not fit u32"))?;
    Ok(Some((pointer.clone(), field)))
}
