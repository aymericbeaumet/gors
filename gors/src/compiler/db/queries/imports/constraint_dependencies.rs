//! Receiver-qualified dependencies needed by generic method constraints.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{
    FunctionSymbols, GenericFunctionSymbol, GenericTypeSymbol, PackageReferences, Ty,
    collect_constraint_expression_method_names, collect_receiver_name_definitions,
};
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, FieldListSyntax};

pub(super) struct ConstraintMethodDependencies {
    pub(super) receivers_by_method: BTreeMap<Arc<str>, BTreeSet<QualifiedDefId>>,
    pub(super) fallback_methods: BTreeSet<Arc<str>>,
}

struct MethodConstrainedParameter {
    index: usize,
    name: Arc<str>,
    methods: BTreeSet<Arc<str>>,
}

pub(super) fn collect(
    references: &PackageReferences,
    symbols: &FunctionSymbols,
    types: &BTreeMap<String, Ty>,
) -> ConstraintMethodDependencies {
    let mut dependencies = ConstraintMethodDependencies {
        receivers_by_method: BTreeMap::new(),
        fallback_methods: BTreeSet::new(),
    };
    for call in &references.generic_calls {
        let Some(generic) = symbols.generic_functions.get(call.callee.as_ref()) else {
            continue;
        };
        collect_call_dependencies(call, generic, symbols, types, &mut dependencies);
    }
    for instantiation in &references.generic_instantiations {
        let Some(generic) = symbols.generic_types.get(instantiation.base.as_ref()) else {
            continue;
        };
        for parameter in
            method_constrained_parameters(&generic.type_parameters, &symbols.generic_types, types)
        {
            record_receiver(
                &parameter.methods,
                instantiation
                    .argument_type_names
                    .get(parameter.index)
                    .and_then(Option::as_deref),
                symbols,
                types,
                &mut dependencies,
            );
        }
    }
    dependencies
}

fn collect_call_dependencies(
    call: &super::super::support::GenericCallReference,
    generic: &GenericFunctionSymbol,
    symbols: &FunctionSymbols,
    types: &BTreeMap<String, Ty>,
    dependencies: &mut ConstraintMethodDependencies,
) {
    let Some(type_parameters) = &generic.header.type_parameters else {
        return;
    };
    for parameter in method_constrained_parameters(type_parameters, &symbols.generic_types, types) {
        if let Some(explicit) = call.explicit_type_argument_names.get(parameter.index) {
            record_receiver(
                &parameter.methods,
                explicit.as_deref(),
                symbols,
                types,
                dependencies,
            );
            continue;
        }
        let argument_slots = direct_parameter_argument_slots(
            &generic.header.params,
            &parameter.name,
            call.argument_type_names.len(),
            call.spread,
        );
        if argument_slots.is_empty() {
            dependencies
                .fallback_methods
                .extend(parameter.methods.iter().cloned());
            continue;
        }
        for slot in argument_slots {
            record_receiver(
                &parameter.methods,
                slot.and_then(|index| call.argument_type_names.get(index))
                    .and_then(Option::as_deref),
                symbols,
                types,
                dependencies,
            );
        }
    }
}

fn method_constrained_parameters(
    fields: &FieldListSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    types: &BTreeMap<String, Ty>,
) -> Vec<MethodConstrainedParameter> {
    let mut result = Vec::new();
    let mut index = 0;
    for field in &*fields.fields {
        let mut methods = BTreeSet::new();
        if let Some(constraint) = &field.ty {
            collect_constraint_expression_method_names(
                constraint,
                generic_types,
                types,
                &mut methods,
                &mut BTreeSet::new(),
            );
        }
        if let Some(names) = &field.names {
            for name in &**names {
                if !methods.is_empty() {
                    result.push(MethodConstrainedParameter {
                        index,
                        name: Arc::clone(&name.name),
                        methods: methods.clone(),
                    });
                }
                index += 1;
            }
        }
    }
    result
}

fn direct_parameter_argument_slots(
    fields: &FieldListSyntax,
    parameter: &str,
    argument_count: usize,
    spread: bool,
) -> Vec<Option<usize>> {
    let mut slots = Vec::new();
    let mut offset = 0;
    for field in &*fields.fields {
        let count = field.names.as_ref().map_or(1, |names| names.len());
        let direct = field
            .ty
            .as_ref()
            .is_some_and(|ty| bare_type_parameter(ty, parameter));
        if direct && field.variadic {
            if spread {
                slots.push(None);
            } else {
                slots.extend((offset..argument_count).map(Some));
            }
            return slots;
        }
        if direct {
            slots.extend(
                (offset..offset + count).map(|index| (index < argument_count).then_some(index)),
            );
        }
        offset += count;
    }
    slots
}

fn bare_type_parameter(expression: &ExprSyntax, parameter: &str) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => bare_type_parameter(inner, parameter),
        ExprSyntaxKind::Ident(ident) => ident.name.as_ref() == parameter,
        _ => false,
    }
}

fn record_receiver(
    methods: &BTreeSet<Arc<str>>,
    name: Option<&str>,
    symbols: &FunctionSymbols,
    types: &BTreeMap<String, Ty>,
    dependencies: &mut ConstraintMethodDependencies,
) {
    let mut definitions = BTreeSet::new();
    if let Some(name) = name {
        collect_receiver_name_definitions(name, types, symbols, &mut definitions);
    }
    if definitions.is_empty() {
        dependencies
            .fallback_methods
            .extend(methods.iter().cloned());
        return;
    }
    for method in methods {
        dependencies
            .receivers_by_method
            .entry(Arc::clone(method))
            .or_default()
            .extend(definitions.iter().copied());
    }
}
