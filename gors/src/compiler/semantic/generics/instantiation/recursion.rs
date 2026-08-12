//! Weighted type-parameter flow for finite generic instantiation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::compiler::Diagnostic;
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::GenericTypeSymbol;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, FieldListSyntax};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Vertex {
    definition: QualifiedDefId,
    parameter: usize,
}

#[derive(Clone, Copy)]
struct Edge {
    source: Vertex,
    destination: Vertex,
    derived: bool,
}

pub(super) fn ensure_finite_instantiation(
    root: QualifiedDefId,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let mut edges = Vec::new();
    for generic in generic_types.values() {
        let parameters = parameter_slots(&generic.type_parameters);
        collect_field_instantiations(
            generic,
            &parameters,
            &generic.type_parameters,
            generic_types,
            &mut edges,
        );
        collect_expression_instantiations(
            generic,
            &parameters,
            &generic.underlying,
            generic_types,
            &mut edges,
        );
    }

    let roots = generic_types
        .values()
        .find(|generic| generic.id == root)
        .map(|generic| {
            (0..parameter_slots(&generic.type_parameters).len())
                .map(|parameter| Vertex {
                    definition: root,
                    parameter,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reachable = reachable_vertices(&roots, &edges);
    let expanding = edges.iter().any(|edge| {
        edge.derived
            && reachable.contains(&edge.source)
            && path_exists(edge.destination, edge.source, &edges)
    });
    if expanding {
        let name = generic_types
            .iter()
            .find_map(|(name, generic)| (generic.id == root).then_some(name.as_str()))
            .unwrap_or("generic type");
        return Err(Diagnostic::semantic(
            format!("expanding recursive generic instantiation involving {name}"),
            source,
        ));
    }
    Ok(())
}

fn collect_field_instantiations(
    owner: &GenericTypeSymbol,
    parameters: &[Option<String>],
    fields: &FieldListSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    edges: &mut Vec<Edge>,
) {
    for field in fields.fields.iter() {
        if let Some(ty) = &field.ty {
            collect_expression_instantiations(owner, parameters, ty, generic_types, edges);
        }
    }
}

fn collect_expression_instantiations(
    owner: &GenericTypeSymbol,
    parameters: &[Option<String>],
    expression: &ExprSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    edges: &mut Vec<Edge>,
) {
    match &expression.kind {
        ExprSyntaxKind::Index { base, index } => {
            record_instantiation(
                owner,
                parameters,
                base,
                std::slice::from_ref(index.as_ref()),
                generic_types,
                edges,
            );
            collect_expression_instantiations(owner, parameters, base, generic_types, edges);
            collect_expression_instantiations(owner, parameters, index, generic_types, edges);
        }
        ExprSyntaxKind::IndexList { base, indices } => {
            record_instantiation(owner, parameters, base, indices, generic_types, edges);
            collect_expression_instantiations(owner, parameters, base, generic_types, edges);
            for index in indices.iter() {
                collect_expression_instantiations(owner, parameters, index, generic_types, edges);
            }
        }
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            expression: inner, ..
        } => {
            collect_expression_instantiations(owner, parameters, inner, generic_types, edges);
        }
        ExprSyntaxKind::Binary { left, right, .. }
        | ExprSyntaxKind::MapType {
            key: left,
            value: right,
        }
        | ExprSyntaxKind::KeyValue {
            key: left,
            value: right,
        } => {
            collect_expression_instantiations(owner, parameters, left, generic_types, edges);
            collect_expression_instantiations(owner, parameters, right, generic_types, edges);
        }
        ExprSyntaxKind::Call {
            callee, arguments, ..
        } => {
            collect_expression_instantiations(owner, parameters, callee, generic_types, edges);
            for argument in arguments.iter() {
                collect_expression_instantiations(
                    owner,
                    parameters,
                    argument,
                    generic_types,
                    edges,
                );
            }
        }
        ExprSyntaxKind::FunctionLiteral {
            params, results, ..
        }
        | ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            collect_field_instantiations(owner, parameters, params, generic_types, edges);
            if let Some(results) = results {
                collect_field_instantiations(owner, parameters, results, generic_types, edges);
            }
        }
        ExprSyntaxKind::Selector { base, .. } => {
            collect_expression_instantiations(owner, parameters, base, generic_types, edges);
        }
        ExprSyntaxKind::TypeAssert { value, asserted } => {
            collect_expression_instantiations(owner, parameters, value, generic_types, edges);
            if let Some(asserted) = asserted {
                collect_expression_instantiations(
                    owner,
                    parameters,
                    asserted,
                    generic_types,
                    edges,
                );
            }
        }
        ExprSyntaxKind::ArrayType { length, element } => {
            if let Some(length) = length {
                collect_expression_instantiations(owner, parameters, length, generic_types, edges);
            }
            collect_expression_instantiations(owner, parameters, element, generic_types, edges);
        }
        ExprSyntaxKind::ChannelType { element, .. } => {
            collect_expression_instantiations(owner, parameters, element, generic_types, edges);
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            collect_field_instantiations(owner, parameters, fields, generic_types, edges);
        }
        ExprSyntaxKind::CompositeLiteral { ty, elements } => {
            if let Some(ty) = ty {
                collect_expression_instantiations(owner, parameters, ty, generic_types, edges);
            }
            for element in elements.iter() {
                collect_expression_instantiations(owner, parameters, element, generic_types, edges);
            }
        }
        ExprSyntaxKind::Slice {
            base,
            low,
            high,
            max,
        } => {
            collect_expression_instantiations(owner, parameters, base, generic_types, edges);
            for bound in [low, high, max].into_iter().flatten() {
                collect_expression_instantiations(owner, parameters, bound, generic_types, edges);
            }
        }
        ExprSyntaxKind::Ident(_)
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Unsupported(_) => {}
    }
}

fn record_instantiation(
    owner: &GenericTypeSymbol,
    parameters: &[Option<String>],
    base: &ExprSyntax,
    arguments: &[ExprSyntax],
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    edges: &mut Vec<Edge>,
) {
    let ExprSyntaxKind::Ident(base) = &base.kind else {
        return;
    };
    let Some(callee) = generic_types.get(base.name.as_ref()) else {
        return;
    };
    for (destination, argument) in arguments.iter().enumerate() {
        let mut referenced = BTreeSet::new();
        collect_referenced_parameters(argument, parameters, &mut referenced);
        let bare = bare_parameter(argument, parameters);
        for source in referenced {
            edges.push(Edge {
                source: Vertex {
                    definition: owner.id,
                    parameter: source,
                },
                destination: Vertex {
                    definition: callee.id,
                    parameter: destination,
                },
                derived: bare != Some(source),
            });
        }
    }
}

fn collect_referenced_parameters(
    expression: &ExprSyntax,
    parameters: &[Option<String>],
    referenced: &mut BTreeSet<usize>,
) {
    if let ExprSyntaxKind::Ident(ident) = &expression.kind {
        for (index, parameter) in parameters.iter().enumerate() {
            if parameter.as_deref() == Some(ident.name.as_ref()) {
                referenced.insert(index);
            }
        }
        return;
    }
    let mut nested = Vec::new();
    collect_children(expression, &mut nested);
    for child in nested {
        collect_referenced_parameters(child, parameters, referenced);
    }
}

fn collect_children<'a>(expression: &'a ExprSyntax, children: &mut Vec<&'a ExprSyntax>) {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            expression: inner, ..
        } => {
            children.push(inner);
        }
        ExprSyntaxKind::Binary { left, right, .. }
        | ExprSyntaxKind::MapType {
            key: left,
            value: right,
        }
        | ExprSyntaxKind::KeyValue {
            key: left,
            value: right,
        } => children.extend([left.as_ref(), right.as_ref()]),
        ExprSyntaxKind::Call {
            callee, arguments, ..
        } => {
            children.push(callee);
            children.extend(arguments.iter());
        }
        ExprSyntaxKind::FunctionLiteral {
            params, results, ..
        }
        | ExprSyntaxKind::FunctionType {
            params, results, ..
        } => collect_field_children(params, children, results.as_ref()),
        ExprSyntaxKind::Selector { base, .. } => children.push(base),
        ExprSyntaxKind::TypeAssert { value, asserted } => {
            children.push(value);
            children.extend(asserted.iter().map(Box::as_ref));
        }
        ExprSyntaxKind::ArrayType { length, element } => {
            children.extend(length.iter().map(Box::as_ref));
            children.push(element);
        }
        ExprSyntaxKind::ChannelType { element, .. } => children.push(element),
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            collect_field_children(fields, children, None);
        }
        ExprSyntaxKind::CompositeLiteral { ty, elements } => {
            children.extend(ty.iter().map(Box::as_ref));
            children.extend(elements.iter());
        }
        ExprSyntaxKind::Index { base, index } => {
            children.extend([base.as_ref(), index.as_ref()]);
        }
        ExprSyntaxKind::IndexList { base, indices } => {
            children.push(base);
            children.extend(indices.iter());
        }
        ExprSyntaxKind::Slice {
            base,
            low,
            high,
            max,
        } => {
            children.push(base);
            children.extend([low, high, max].into_iter().flatten().map(Box::as_ref));
        }
        ExprSyntaxKind::Ident(_)
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Unsupported(_) => {}
    }
}

fn collect_field_children<'a>(
    fields: &'a FieldListSyntax,
    children: &mut Vec<&'a ExprSyntax>,
    results: Option<&'a FieldListSyntax>,
) {
    children.extend(fields.fields.iter().filter_map(|field| field.ty.as_ref()));
    if let Some(results) = results {
        children.extend(results.fields.iter().filter_map(|field| field.ty.as_ref()));
    }
}

fn bare_parameter(expression: &ExprSyntax, parameters: &[Option<String>]) -> Option<usize> {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => bare_parameter(inner, parameters),
        ExprSyntaxKind::Ident(ident) => parameters
            .iter()
            .position(|parameter| parameter.as_deref() == Some(ident.name.as_ref())),
        _ => None,
    }
}

fn parameter_slots(fields: &FieldListSyntax) -> Vec<Option<String>> {
    fields
        .fields
        .iter()
        .flat_map(|field| field.names.iter().flat_map(|names| names.iter()))
        .map(|name| (name.name.as_ref() != "_").then(|| name.name.to_string()))
        .collect()
}

fn reachable_vertices(roots: &[Vertex], edges: &[Edge]) -> BTreeSet<Vertex> {
    let mut reachable = roots.iter().copied().collect::<BTreeSet<_>>();
    let mut queue = roots.iter().copied().collect::<VecDeque<_>>();
    while let Some(vertex) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| edge.source == vertex) {
            if reachable.insert(edge.destination) {
                queue.push_back(edge.destination);
            }
        }
    }
    reachable
}

fn path_exists(start: Vertex, target: Vertex, edges: &[Edge]) -> bool {
    let mut seen = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(vertex) = queue.pop_front() {
        if vertex == target {
            return true;
        }
        for edge in edges.iter().filter(|edge| edge.source == vertex) {
            if seen.insert(edge.destination) {
                queue.push_back(edge.destination);
            }
        }
    }
    false
}
