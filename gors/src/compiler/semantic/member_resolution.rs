//! Shared shallowest-depth field and method resolution.

use std::collections::{BTreeMap, BTreeSet};

use super::MethodSymbol;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::DefId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{StructField, Ty};

#[derive(Clone, Copy)]
pub(super) enum MethodLookup {
    /// Ordinary `x.M` selector shorthand may take the address of an
    /// addressable value in addition to the formal method set rules.
    Selector { addressable: bool },
    /// Exact Go method-set membership, without selector shorthand.
    MethodSet,
}

#[derive(Clone)]
pub(super) enum ResolvedMember {
    Field(FieldResolution),
    Method(MethodResolution),
}

#[derive(Clone)]
pub(super) struct FieldResolution {
    pub(super) path: Vec<hir::EmbeddedFieldStep>,
}

#[derive(Clone)]
pub(super) struct MethodResolution {
    pub(super) symbol: MethodSymbol,
    pub(super) plan: hir::MethodReceiverPlan,
}

#[derive(Clone)]
struct SearchNode {
    ty: Ty,
    path: Vec<hir::EmbeddedFieldStep>,
    /// A pointer root or an embedded pointer makes pointer-receiver methods
    /// members of the formal promoted method set.
    indirect: bool,
    /// A defined pointer type keeps field-selector shorthand through its
    /// underlying pointer, but it does not inherit that pointer target's
    /// methods or promoted method set.
    methods_visible: bool,
    seen: BTreeSet<DefId>,
}

/// Resolve a selector using the one Go shallowest-depth namespace shared by
/// fields and methods. Equal-depth candidates are ambiguous, even if they
/// reach the same declaration through two different embedding paths.
pub(super) fn resolve_selector_member(
    root_ty: &Ty,
    name: &str,
    methods: &BTreeMap<(DefId, String), MethodSymbol>,
    lookup: MethodLookup,
    source: SourceRef,
) -> Result<ResolvedMember, Diagnostic> {
    let root_indirect = matches!(root_ty.underlying(), Ty::Pointer(_));
    let methods_visible = !is_defined_pointer(root_ty);
    let mut seen = BTreeSet::new();
    if let Some(definition) = receiver_definition(root_ty) {
        seen.insert(definition);
    }
    let mut level = vec![SearchNode {
        ty: root_ty.clone(),
        path: Vec::new(),
        indirect: root_indirect,
        methods_visible,
        seen,
    }];

    while !level.is_empty() {
        let mut matches = Vec::new();
        for node in &level {
            collect_direct_matches(node, name, methods, lookup, &mut matches)?;
        }
        if matches.len() == 1 {
            return matches
                .pop()
                .ok_or_else(|| Diagnostic::backend("member resolution lost its sole result"));
        }
        if matches.len() > 1 {
            return Err(Diagnostic::semantic(
                format!("selector {name} is ambiguous"),
                source,
            ));
        }

        let mut next = Vec::new();
        for node in level {
            let Some(fields) = struct_fields(&node.ty) else {
                continue;
            };
            for (index, field) in fields
                .iter()
                .enumerate()
                .filter(|(_, field)| field.embedded)
            {
                let mut seen = node.seen.clone();
                if let Some(definition) = receiver_definition(&field.ty)
                    && !seen.insert(definition)
                {
                    continue;
                }
                let field_index = u32::try_from(index)
                    .map_err(|_| Diagnostic::backend("struct exceeds the field index domain"))?;
                let mut path = node.path.clone();
                path.push(hir::EmbeddedFieldStep {
                    owner_ty: node.ty.clone(),
                    field: field_index,
                    field_ty: field.ty.clone(),
                });
                next.push(SearchNode {
                    ty: field.ty.clone(),
                    path,
                    indirect: node.indirect || matches!(field.ty.underlying(), Ty::Pointer(_)),
                    methods_visible: node.methods_visible,
                    seen,
                });
            }
        }
        level = next;
    }

    Err(Diagnostic::semantic(
        format!("type {root_ty:?} has no field or method {name}"),
        source,
    ))
}

/// Resolve one exact member of the formal Go method set. Fields still
/// participate in the shallowest-depth namespace and can suppress or make a
/// promoted method ambiguous.
pub(super) fn resolve_method_set_member(
    root_ty: &Ty,
    name: &str,
    methods: &BTreeMap<(DefId, String), MethodSymbol>,
    source: SourceRef,
) -> Result<MethodResolution, Diagnostic> {
    match resolve_selector_member(root_ty, name, methods, MethodLookup::MethodSet, source)? {
        ResolvedMember::Method(method) => Ok(method),
        ResolvedMember::Field(_) => Err(Diagnostic::semantic(
            format!("type {root_ty:?} has no method {name}"),
            source,
        )),
    }
}

fn collect_direct_matches(
    node: &SearchNode,
    name: &str,
    methods: &BTreeMap<(DefId, String), MethodSymbol>,
    lookup: MethodLookup,
    matches: &mut Vec<ResolvedMember>,
) -> Result<(), Diagnostic> {
    if let Some(fields) = struct_fields(&node.ty) {
        for (index, field) in fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field.name == name)
        {
            let field_index = u32::try_from(index)
                .map_err(|_| Diagnostic::backend("struct exceeds the field index domain"))?;
            let mut path = node.path.clone();
            path.push(hir::EmbeddedFieldStep {
                owner_ty: node.ty.clone(),
                field: field_index,
                field_ty: field.ty.clone(),
            });
            matches.push(ResolvedMember::Field(FieldResolution { path }));
        }
    }

    if !node.methods_visible {
        return Ok(());
    }
    let Some(definition) = receiver_definition(&node.ty) else {
        return Ok(());
    };
    let Some(symbol) = methods.get(&(definition, name.to_owned())) else {
        return Ok(());
    };
    let pointer_allowed =
        node.indirect || matches!(lookup, MethodLookup::Selector { addressable: true });
    if symbol.pointer_receiver && !pointer_allowed {
        return Ok(());
    }
    let receiver_ty = symbol
        .signature
        .params
        .first()
        .cloned()
        .ok_or_else(|| Diagnostic::backend("method signature omitted its receiver"))?;
    let adjustment = receiver_adjustment(&node.ty, &receiver_ty)?;
    matches.push(ResolvedMember::Method(MethodResolution {
        symbol: symbol.clone(),
        plan: hir::MethodReceiverPlan {
            root_ty: node
                .path
                .first()
                .map_or_else(|| node.ty.clone(), |step| step.owner_ty.clone()),
            path: node.path.clone(),
            selected_ty: node.ty.clone(),
            adjustment,
            receiver_ty,
        },
    }));
    Ok(())
}

fn receiver_adjustment(
    selected_ty: &Ty,
    receiver_ty: &Ty,
) -> Result<hir::MethodReceiverAdjustment, Diagnostic> {
    if selected_ty == receiver_ty {
        return Ok(hir::MethodReceiverAdjustment::Identity);
    }
    let selected_definition = receiver_definition(selected_ty);
    let receiver_definition = receiver_definition(receiver_ty);
    if selected_definition != receiver_definition || selected_definition.is_none() {
        return Err(Diagnostic::backend(format!(
            "method receiver selection changed its named type from {selected_ty:?} to {receiver_ty:?}"
        )));
    }
    match (
        matches!(selected_ty.underlying(), Ty::Pointer(_)),
        matches!(receiver_ty.underlying(), Ty::Pointer(_)),
    ) {
        (false, true) => Ok(hir::MethodReceiverAdjustment::AutoAddress),
        (true, false) => Ok(hir::MethodReceiverAdjustment::AutoIndirect),
        _ => Err(Diagnostic::backend(format!(
            "method receiver selection cannot adapt {selected_ty:?} to {receiver_ty:?}"
        ))),
    }
}

pub(super) fn struct_fields(ty: &Ty) -> Option<&[StructField]> {
    match ty.underlying() {
        Ty::Struct(fields) => Some(fields),
        Ty::Pointer(element) => match element.underlying() {
            Ty::Struct(fields) => Some(fields),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn receiver_definition(ty: &Ty) -> Option<DefId> {
    match ty {
        Ty::Named { definition, .. } | Ty::NamedRef { definition } => Some(*definition),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } | Ty::NamedRef { definition } => Some(*definition),
            _ => None,
        },
        _ => None,
    }
}

fn is_defined_pointer(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Named { underlying, .. } if matches!(underlying.underlying(), Ty::Pointer(_))
    )
}
