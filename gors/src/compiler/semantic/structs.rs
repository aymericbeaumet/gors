//! Exact struct literal and direct-field expression lowering.

use super::expressions::{coerce_expr, ensure_bootstrap_value_type};
use super::pointers::pointer_effects;
use super::{FunctionLowerer, MethodSymbol};
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
            && !self.variables.contains_key(package.name.as_ref())
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
        let receiver = self.lower_expr(base, None)?;
        if matches!(receiver.ty.underlying(), Ty::Interface(_)) {
            return self.lower_interface_selector_call(
                receiver,
                &member.name,
                member.source,
                arguments,
                spread,
                node,
                source,
                expected,
                allow_discarded_call_result,
            );
        }
        let symbol = self.resolve_method_symbol(&receiver.ty, &member.name, source)?;
        let Some((receiver_ty, params)) = symbol.signature.params.split_first() else {
            return Err(Diagnostic::backend("method signature omitted its receiver"));
        };
        let receiver = self.adjust_method_receiver(
            receiver,
            receiver_ty,
            symbol.pointer_receiver,
            base.source,
            source,
        )?;
        let lowered_arguments = self.lower_call_arguments(
            arguments,
            params,
            symbol.signature.variadic,
            spread,
            member.source,
            source,
            "method",
        )?;
        let mut args = Vec::with_capacity(lowered_arguments.len().saturating_add(1));
        args.push(receiver);
        args.extend(lowered_arguments);
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

    pub(super) fn resolve_method_symbol(
        &self,
        receiver: &Ty,
        name: &str,
        source: SourceRef,
    ) -> Result<MethodSymbol, Diagnostic> {
        let definition = named_receiver_definition(receiver).ok_or_else(|| {
            Diagnostic::semantic(format!("type {receiver:?} has no method {name}"), source)
        })?;
        self.methods
            .get(&(definition, name.to_owned()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::semantic(format!("type {receiver:?} has no method {name}"), source)
            })
    }

    pub(super) fn adjust_method_receiver(
        &mut self,
        mut receiver: hir::Expr,
        receiver_ty: &Ty,
        pointer_receiver: bool,
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        if receiver.ty == *receiver_ty {
            coerce_expr(&mut receiver, receiver_ty, source)?;
            return Ok(receiver);
        }
        if pointer_receiver
            && let Ty::Pointer(element) = receiver_ty.underlying()
            && receiver.ty == **element
            && let hir::ExprKind::Local(local) = &receiver.kind
        {
            let node = self.alloc_node(syntax_source)?;
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::AddressOfLocal(*local),
                ty: receiver_ty.clone(),
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_allocate: true,
                    may_read: true,
                    ..hir::Effects::default()
                },
                source: SourceRef::node(node),
            });
        }
        if !pointer_receiver
            && receiver_ty.bootstrap_i64_struct_fields().is_some()
            && let Ty::Pointer(element) = receiver.ty.underlying()
            && **element == *receiver_ty
        {
            let effects = pointer_effects(&[&receiver], false, false, true);
            let node = self.alloc_node(syntax_source)?;
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::PointerStructValue(Box::new(receiver)),
                ty: receiver_ty.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source: SourceRef::node(node),
            });
        }
        coerce_expr(&mut receiver, receiver_ty, source)?;
        Ok(receiver)
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
            && !self.variables.contains_key(package.name.as_ref())
        {
            return self.lower_imported_selector(base, member, node, source);
        }
        let structure = self.lower_expr(base, None)?;
        ensure_bootstrap_value_type(&structure.ty, source)?;
        let pointer_structure = structure.ty.bootstrap_i64_struct_pointer_fields().is_some();
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
        let effects = if pointer_structure {
            pointer_effects(&[&structure], false, false, true)
        } else {
            structure.effects.union(hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            })
        };
        let kind = if pointer_structure {
            let index_node = self.alloc_node(member.source)?;
            hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Get),
                args: vec![
                    structure,
                    hir::Expr {
                        node: index_node,
                        kind: hir::ExprKind::Constant(crate::compiler::types::ConstValue::Int(
                            field.to_string(),
                        )),
                        ty: Ty::Int(crate::compiler::types::IntTy::Int),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source: SourceRef::node(index_node),
                    },
                ],
            }
        } else {
            hir::ExprKind::StructField {
                structure: Box::new(structure),
                field,
            }
        };
        Ok(hir::Expr {
            node,
            kind,
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
    match ty.underlying() {
        Ty::Struct(fields) => Some(fields),
        Ty::Pointer(element) => element.bootstrap_i64_struct_fields(),
        _ => None,
    }
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
