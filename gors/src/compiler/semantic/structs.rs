//! Exact struct literal and direct-field expression lowering.

use super::FunctionLowerer;
use super::expressions::{coerce_expr, ensure_bootstrap_value_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, IdentSyntax, SyntaxSource};
use crate::compiler::types::{StructField, Ty};

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_selector_call(
        &mut self,
        base: &ExprSyntax,
        member: &IdentSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        if let ExprSyntaxKind::Ident(package) = &base.kind
            && self.lookup_local(&package.name).is_none()
        {
            return self.lower_imported_selector_call(
                base,
                member,
                arguments,
                spread,
                node,
                source,
                expected,
                allow_discarded_call_result,
            );
        }
        let mut receiver = self.lower_expr(base, None)?;
        let definition = named_receiver_definition(&receiver.ty).ok_or_else(|| {
            Diagnostic::semantic(
                format!("type {:?} has no method {}", receiver.ty, member.name),
                source,
            )
        })?;
        let symbol = self
            .methods
            .get(&(definition, member.name.to_string()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("type {:?} has no method {}", receiver.ty, member.name),
                    source,
                )
            })?;
        if symbol.pointer_receiver {
            return Err(Diagnostic::unsupported(
                "pointer-receiver method calls are not yet represented",
                source,
            ));
        }
        let Some((receiver_ty, params)) = symbol.signature.params.split_first() else {
            return Err(Diagnostic::backend("method signature omitted its receiver"));
        };
        if spread && !symbol.signature.variadic {
            return Err(Diagnostic::semantic(
                "... is only valid when calling a variadic method",
                source,
            ));
        }
        if symbol.signature.variadic && !spread {
            return Err(Diagnostic::unsupported(
                "individual variadic arguments require slice-pack lowering",
                source,
            ));
        }
        if arguments.len() != params.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "method call has {} arguments; expected {}",
                    arguments.len(),
                    params.len()
                ),
                source,
            ));
        }
        coerce_expr(&mut receiver, receiver_ty, source)?;
        let mut args = Vec::with_capacity(arguments.len().saturating_add(1));
        args.push(receiver);
        args.extend(
            arguments
                .iter()
                .zip(params)
                .map(|(argument, expected)| self.lower_expr(argument, Some(expected)))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let ty = match symbol.signature.results.as_slice() {
            [] => Ty::Unit,
            [single] => single.clone(),
            many => Ty::Tuple(many.to_vec()),
        };
        if ty == Ty::Unit && !allow_discarded_call_result {
            return Err(Diagnostic::unsupported(
                "a no-result method call cannot be used as a value",
                source,
            ));
        }
        let effects = args.iter().fold(
            hir::Effects {
                may_call: true,
                may_allocate: true,
                may_block: true,
                may_panic: true,
                may_write: true,
                ..hir::Effects::default()
            },
            |effects, argument| effects.union(argument.effects),
        );
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Function(symbol.id),
                args,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }

    pub(super) fn lower_selector(
        &mut self,
        base: &ExprSyntax,
        member: &IdentSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        if let ExprSyntaxKind::Ident(package) = &base.kind
            && self.lookup_local(&package.name).is_none()
        {
            return self.lower_imported_selector(base, member, node, source);
        }
        let structure = self.lower_expr(base, None)?;
        ensure_bootstrap_value_type(&structure.ty, source)?;
        let fields = struct_fields(&structure.ty).ok_or_else(|| {
            Diagnostic::semantic(
                format!("type {:?} has no field {}", structure.ty, member.name),
                source,
            )
        })?;
        let (field, definition) = fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == member.name.as_ref())
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("type {:?} has no field {}", structure.ty, member.name),
                    source,
                )
            })?;
        let field = u32::try_from(field)
            .map_err(|_| Diagnostic::backend("struct exceeds the field index domain"))?;
        let field_ty = definition.ty.clone();
        let effects = structure.effects.union(hir::Effects {
            may_read: true,
            ..hir::Effects::default()
        });
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::StructField {
                structure: Box::new(structure),
                field,
            },
            ty: field_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn lower_struct_literal(
        &mut self,
        literal_ty: Ty,
        elements: &[ExprSyntax],
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        ensure_bootstrap_value_type(&literal_ty, source)?;
        let fields = struct_fields(&literal_ty)
            .ok_or_else(|| Diagnostic::backend("struct literal has a non-struct type"))?;
        let mut initializers = vec![None; fields.len()];
        let keyed = elements
            .first()
            .is_some_and(|element| matches!(element.kind, ExprSyntaxKind::KeyValue { .. }));
        if keyed {
            for element in elements {
                let ExprSyntaxKind::KeyValue { key, value } = &element.kind else {
                    return Err(Diagnostic::semantic(
                        "mixture of keyed and unkeyed struct literal elements",
                        source,
                    ));
                };
                let ExprSyntaxKind::Ident(key) = &key.kind else {
                    return Err(Diagnostic::semantic(
                        "struct literal field key must be an identifier",
                        source,
                    ));
                };
                let index = fields
                    .iter()
                    .position(|field| field.name == key.name.as_ref())
                    .ok_or_else(|| {
                        Diagnostic::semantic(
                            format!("unknown field {} in struct literal", key.name),
                            source,
                        )
                    })?;
                let initializer = initializers.get_mut(index).ok_or_else(|| {
                    Diagnostic::backend("resolved struct field index is out of bounds")
                })?;
                if initializer.replace(value.as_ref()).is_some() {
                    return Err(Diagnostic::semantic(
                        format!("duplicate field {} in struct literal", key.name),
                        source,
                    ));
                }
            }
        } else if !elements.is_empty() {
            if elements.len() != fields.len() {
                return Err(Diagnostic::semantic(
                    format!(
                        "struct literal has {} values for {} fields",
                        elements.len(),
                        fields.len()
                    ),
                    source,
                ));
            }
            for (destination, value) in initializers.iter_mut().zip(elements) {
                *destination = Some(value);
            }
        }

        let mut values = Vec::with_capacity(fields.len());
        for (field, initializer) in fields.iter().zip(initializers) {
            values.push(match initializer {
                Some(initializer) => self.lower_expr(initializer, Some(&field.ty))?,
                None => self.zero_struct_field(&field.ty, syntax_source)?,
            });
        }
        let effects = values
            .iter()
            .fold(hir::Effects::default(), |effects, value| {
                effects.union(value.effects)
            });
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::StructLiteral(values),
            ty: literal_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    fn zero_struct_field(
        &mut self,
        ty: &Ty,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let value = ty.zero().ok_or_else(|| {
            Diagnostic::unsupported(
                format!("zero value for struct field type {ty:?} is not yet implemented"),
                SourceRef::definition(self.owner),
            )
        })?;
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Constant(value),
            ty: ty.clone(),
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(node),
        })
    }
}

fn struct_fields(ty: &Ty) -> Option<&[StructField]> {
    let Ty::Struct(fields) = ty.underlying() else {
        return None;
    };
    Some(fields)
}

fn named_receiver_definition(ty: &Ty) -> Option<crate::compiler::ids::DefId> {
    match ty {
        Ty::Named { definition, .. } => Some(*definition),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => Some(*definition),
            _ => None,
        },
        _ => None,
    }
}
