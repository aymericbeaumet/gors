//! Exact struct literal and direct-field expression lowering.

use std::collections::HashSet;

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
        if let Some(lowered) = self.try_lower_generic_method_call(
            receiver.clone(),
            &member.name,
            arguments,
            spread,
            node,
            member.source,
            source,
            expected,
            allow_discarded_call_result,
        )? {
            return Ok(lowered);
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
        let effects = hir::Effects {
            may_call: true,
            may_allocate: true,
            may_block: true,
            may_panic: true,
            may_write: true,
            ..hir::Effects::default()
        }
        .union(receiver.effects)
        .union(lowered_arguments.effects());
        let mut lowered = hir::Expr {
            node,
            kind: lowered_arguments
                .into_call_kind(hir::Callee::Function(symbol.id), vec![receiver]),
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
            && (receiver_ty.bootstrap_i64_struct_fields().is_some()
                || receiver_ty.uses_interface_aggregate_pointer_representation())
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
        let mut structure = self.lower_expr(base, None)?;
        ensure_bootstrap_value_type(&structure.ty, source)?;
        let paths = promoted_field_paths(&structure.ty, member.name.as_ref(), &mut HashSet::new());
        let Some(minimum_depth) = paths.iter().map(|(path, _)| path.len()).min() else {
            return Err(Diagnostic::semantic(
                format!("type {:?} has no field {}", structure.ty, member.name),
                source,
            ));
        };
        let mut nearest = paths
            .into_iter()
            .filter(|(path, _)| path.len() == minimum_depth);
        let (path, _) = nearest.next().ok_or_else(|| {
            Diagnostic::backend("promoted struct field search lost its nearest result")
        })?;
        if nearest.next().is_some() {
            return Err(Diagnostic::semantic(
                format!("selector {} is ambiguous", member.name),
                source,
            ));
        }
        for (position, field) in path.iter().copied().enumerate() {
            let fields = struct_fields(&structure.ty).ok_or_else(|| {
                Diagnostic::backend("promoted struct field path crossed a non-struct type")
            })?;
            let field_ty = fields
                .get(field)
                .map(|definition| definition.ty.clone())
                .ok_or_else(|| {
                    Diagnostic::backend("promoted struct field path is out of bounds")
                })?;
            let last = position + 1 == path.len();
            let step_node = if last {
                node
            } else {
                self.alloc_node(member.source)?
            };
            let step_source = if last {
                source
            } else {
                SourceRef::node(step_node)
            };
            structure = self.lower_struct_field_step(
                structure,
                u32::try_from(field)
                    .map_err(|_| Diagnostic::backend("struct exceeds the field index domain"))?,
                field_ty,
                step_node,
                step_source,
                member.source,
            )?;
        }
        Ok(structure)
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_struct_field_step(
        &mut self,
        structure: hir::Expr,
        field: u32,
        field_ty: Ty,
        node: NodeId,
        source: SourceRef,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let pointer_structure = structure.ty.bootstrap_i64_struct_pointer_fields().is_some();
        let aggregate_pointer_structure = matches!(
            structure.ty.underlying(),
            Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation()
        );
        let effects = if pointer_structure || aggregate_pointer_structure {
            pointer_effects(&[&structure], false, false, true)
        } else {
            structure.effects.union(hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            })
        };
        let kind = if pointer_structure {
            let index_node = self.alloc_node(syntax_source)?;
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
        } else if aggregate_pointer_structure {
            let Ty::Pointer(element) = structure.ty.underlying() else {
                return Err(Diagnostic::backend(
                    "aggregate pointer selector lost its pointer type",
                ));
            };
            let element_ty = element.as_ref().clone();
            let dereference_node = self.alloc_node(syntax_source)?;
            let dereference = hir::Expr {
                node: dereference_node,
                kind: hir::ExprKind::PointerStructValue(Box::new(structure)),
                ty: element_ty,
                category: hir::ValueCategory::Value,
                effects,
                source: SourceRef::node(dereference_node),
            };
            hir::ExprKind::StructField {
                structure: Box::new(dereference),
                field,
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
        let node = self.alloc_node(syntax_source)?;
        self.zero_value_expr(node, SourceRef::node(node), ty.clone())
    }
}

fn promoted_field_paths(
    ty: &Ty,
    name: &str,
    seen: &mut HashSet<crate::compiler::ids::DefId>,
) -> Vec<(Vec<usize>, Ty)> {
    let owner = field_owner_definition(ty);
    if owner.is_some_and(|owner| !seen.insert(owner)) {
        return Vec::new();
    }
    let Some(fields) = struct_fields(ty) else {
        if let Some(owner) = owner {
            seen.remove(&owner);
        }
        return Vec::new();
    };
    let direct = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == name)
        .map(|(index, field)| (vec![index], field.ty.clone()))
        .collect::<Vec<_>>();
    if !direct.is_empty() {
        if let Some(owner) = owner {
            seen.remove(&owner);
        }
        return direct;
    }
    let mut promoted = Vec::new();
    for (index, field) in fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.embedded)
    {
        for (path, field_ty) in promoted_field_paths(&field.ty, name, seen) {
            let mut candidate = Vec::with_capacity(path.len().saturating_add(1));
            candidate.push(index);
            candidate.extend(path);
            promoted.push((candidate, field_ty));
        }
    }
    if let Some(owner) = owner {
        seen.remove(&owner);
    }
    promoted
}

fn field_owner_definition(ty: &Ty) -> Option<crate::compiler::ids::DefId> {
    match ty {
        Ty::Named { definition, .. } => Some(*definition),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => Some(*definition),
            _ => None,
        },
        _ => None,
    }
}

fn struct_fields(ty: &Ty) -> Option<&[StructField]> {
    match ty.underlying() {
        Ty::Struct(fields) => Some(fields),
        Ty::Pointer(element) => match element.underlying() {
            Ty::Struct(fields) => Some(fields),
            _ => None,
        },
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
