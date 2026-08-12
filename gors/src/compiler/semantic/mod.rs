//! Definition-demanded name resolution and type checking over owned syntax.

mod arrays;
mod assignment_targets;
mod assignments;
mod call_expressions;
mod calls;
mod channels;
mod closures;
mod composites;
mod constant_ops;
mod constants;
mod control_targets;
mod conversions;
mod declarations;
mod expression_lower;
mod expressions;
mod function;
mod generics;
mod goroutines;
mod goto_scopes;
mod imports;
mod interfaces;
mod iteration;
mod length_capacity;
mod maps;
mod member_resolution;
mod numeric_builtins;
mod pointers;
mod ranges;
mod recovery;
mod selects;
mod shifts;
mod slices;
mod statements;
mod static_values;
mod structs;
mod switches;
mod type_lowering;
mod type_switches;
mod unsafe_intrinsics;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(super) use arrays::array_length_from_constant;
pub(super) use constants::{
    eval_constant, eval_constant_with_lookup, validate_constant_binary_operator,
};
use expressions::*;
use function::FunctionLowerer;
pub(in crate::compiler) use generics::lower_type_with_generic_symbols;
use type_lowering::{
    field_types, field_types_with_constant_lookup, parameter_types,
    parameter_types_with_constant_lookup,
};
pub(super) use type_lowering::{
    lower_type, lower_type_with_constant_lookup, lower_type_with_constants,
};

use super::Diagnostic;
use super::hir;
use super::ids::{DefId, NodeId, QualifiedDefId};
use super::provenance::SourceRef;
use super::syntax::{
    ConstantSyntax, ConstantValueSyntax, ExprSyntax, FieldListSyntax, FunctionBodySyntax,
    FunctionHeaderSyntax, SyntaxSource, VariableSyntax, VariableValueSyntax,
};
use super::types::{ConstValue, Signature, StaticValue, Ty};

#[derive(Clone)]
pub(super) struct FunctionSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) signature: Signature,
    pub(super) range_header: Option<Arc<FunctionHeaderSyntax>>,
    pub(super) range_body: Option<Arc<FunctionBodySyntax>>,
}

#[derive(Clone)]
pub(super) struct MethodSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) signature: Signature,
    pub(super) pointer_receiver: bool,
}

#[derive(Clone)]
pub(super) struct GenericFunctionSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) header: Arc<FunctionHeaderSyntax>,
    pub(super) body: Arc<FunctionBodySyntax>,
    pub(super) pointer_receiver: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GenericTypeSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) type_parameters: Arc<FieldListSyntax>,
    pub(super) underlying: ExprSyntax,
    pub(super) alias: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ConstantSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone)]
pub(super) struct VariableSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) ty: Ty,
    pub(super) value: StaticValue,
}

pub(super) struct FunctionSymbols {
    pub(super) functions: BTreeMap<String, FunctionSymbol>,
    pub(super) qualified_functions: BTreeMap<(String, String), FunctionSymbol>,
    pub(super) methods: BTreeMap<(QualifiedDefId, String), MethodSymbol>,
    pub(super) generic_functions: BTreeMap<String, GenericFunctionSymbol>,
    pub(super) generic_methods: BTreeMap<(QualifiedDefId, String), GenericFunctionSymbol>,
    pub(super) generic_types: BTreeMap<String, GenericTypeSymbol>,
    pub(super) constants: BTreeMap<String, ConstantSymbol>,
    pub(super) qualified_constants: BTreeMap<(String, String), ConstantSymbol>,
    pub(super) variables: BTreeMap<String, VariableSymbol>,
    pub(super) qualified_variables: BTreeMap<(String, String), VariableSymbol>,
    pub(super) package_imports: BTreeSet<String>,
    pub(super) intrinsic_packages: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TypedConstant {
    pub(super) id: DefId,
    pub(super) name: String,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TypedVariable {
    pub(super) id: DefId,
    pub(super) name: String,
    pub(super) ty: Ty,
    pub(super) value: StaticValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoweredFunction {
    pub(super) function: hir::Function,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionLoweringFailure {
    pub(super) diagnostic: Diagnostic,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

pub(super) fn lower_signature(
    definition: DefId,
    header: &FunctionHeaderSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constants: &BTreeMap<String, ConstantSymbol>,
) -> Result<Signature, Diagnostic> {
    let source = SourceRef::definition(definition);
    let constant_lookup = |name: &str| {
        constants
            .get(name)
            .map(|constant| (constant.ty.clone(), constant.value.clone()))
    };
    if header.has_type_parameters {
        return Err(Diagnostic::unsupported(
            "generic functions with this declaration form are not yet supported",
            source,
        ));
    }
    let mut params = Vec::new();
    if let Some(receiver) = &header.receiver {
        let (receiver, receiver_variadic) =
            parameter_types_with_constant_lookup(receiver, type_aliases, &constant_lookup, source)?;
        if receiver_variadic || receiver.len() != 1 {
            return Err(Diagnostic::semantic(
                "a method must declare exactly one non-variadic receiver",
                source,
            ));
        }
        params.extend(receiver);
    }
    let (ordinary_params, variadic) = parameter_types_with_constant_lookup(
        &header.params,
        type_aliases,
        &constant_lookup,
        source,
    )?;
    params.extend(ordinary_params);
    let results = header
        .results
        .as_ref()
        .map(|fields| {
            field_types_with_constant_lookup(fields, type_aliases, &constant_lookup, source)
        })
        .transpose()?
        .unwrap_or_default();
    for ty in params.iter().chain(&results) {
        ensure_bootstrap_value_type(ty, source)?;
    }
    if header.receiver.is_none()
        && header.name.name.as_ref() == "main"
        && (!params.is_empty() || !results.is_empty())
    {
        return Err(Diagnostic::semantic(
            "func main must have no parameters and no results",
            source,
        ));
    }
    Ok(Signature {
        params,
        results,
        variadic,
    })
}

pub(super) fn lower_constant_with_variables(
    definition: DefId,
    syntax: &ConstantSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    type_aliases: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
) -> Result<TypedConstant, Diagnostic> {
    let source = SourceRef::definition(definition);
    let expression = match &syntax.value {
        ConstantValueSyntax::Expression(expression) => expression,
        ConstantValueSyntax::ImplicitOrIota => {
            return Err(Diagnostic::unsupported(
                "implicit repeated const expressions and iota are not implemented",
                source,
            ));
        }
        ConstantValueSyntax::ArityMismatch => {
            return Err(Diagnostic::unsupported(
                "multi-valued const expressions are not implemented",
                source,
            ));
        }
    };
    let (raw_ty, value) = constants::eval_constant_with_variables(
        expression,
        constants,
        variables,
        type_aliases,
        shadowed_predeclared,
        source,
        Some(syntax.iota),
    )?;
    let ty = syntax
        .explicit_type
        .as_ref()
        .map(|ty| lower_type(ty, type_aliases, source))
        .transpose()?
        .unwrap_or_else(|| raw_ty.clone());
    ensure_bootstrap_value_type(&ty.default_typed(), source)?;
    if !is_assignable(&raw_ty, &ty) {
        return Err(Diagnostic::semantic(
            format!("constant {} is not assignable to {ty:?}", syntax.name.name),
            source,
        ));
    }
    if !value.is_representable_as(&ty) {
        return Err(Diagnostic::semantic(
            format!(
                "constant {} is not representable as {ty:?}",
                syntax.name.name
            ),
            source,
        ));
    }
    let value = value.normalized_for(&ty);
    Ok(TypedConstant {
        id: definition,
        name: syntax.name.name.to_string(),
        ty,
        value,
    })
}

pub(super) fn lower_variable(
    definition: DefId,
    syntax: &VariableSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    type_aliases: &BTreeMap<String, Ty>,
    static_functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    package_initializers: &[Arc<FunctionBodySyntax>],
) -> Result<TypedVariable, Diagnostic> {
    let source = SourceRef::definition(definition);
    let explicit_ty = syntax
        .explicit_type
        .as_ref()
        .map(|ty| lower_type_with_constants(ty, type_aliases, constants, source))
        .transpose()?;
    let (raw_ty, mut value) = match &syntax.value {
        VariableValueSyntax::Expression(expression) => static_values::evaluate_initializer(
            expression,
            constants,
            type_aliases,
            static_functions,
            source,
        )?,
        VariableValueSyntax::Zero => {
            let ty = explicit_ty.clone().ok_or_else(|| {
                Diagnostic::semantic(
                    format!(
                        "variable {} has neither a type nor an initializer",
                        syntax.name.name
                    ),
                    source,
                )
            })?;
            let value = StaticValue::zero(&ty).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!("zero value for package variable type {ty:?} is not implemented"),
                    source,
                )
            })?;
            (ty, value)
        }
        VariableValueSyntax::ArityMismatch => {
            return Err(Diagnostic::unsupported(
                "multi-valued package variable initializers are not yet represented",
                source,
            ));
        }
    };
    let ty = explicit_ty.unwrap_or_else(|| raw_ty.default_typed());
    ensure_bootstrap_value_type(&ty, source)?;
    if !is_assignable(&raw_ty, &ty) || !value.is_representable_as(&ty) {
        return Err(Diagnostic::semantic(
            format!(
                "initializer for package variable {} is not assignable to {ty:?}",
                syntax.name.name
            ),
            source,
        ));
    }
    static_values::apply_package_initializers(
        syntax.name.name.as_ref(),
        &ty,
        &mut value,
        package_initializers,
        constants,
        type_aliases,
        static_functions,
        source,
    )?;
    if !value.is_representable_as(&ty) {
        return Err(Diagnostic::backend(format!(
            "initialized package variable {} no longer matches {ty:?}",
            syntax.name.name
        )));
    }
    Ok(TypedVariable {
        id: definition,
        name: syntax.name.name.to_string(),
        ty,
        value,
    })
}

pub(super) fn lower_function(
    definition: DefId,
    header: &FunctionHeaderSyntax,
    body: &FunctionBodySyntax,
    signature: Signature,
    symbols: FunctionSymbols,
    type_aliases: BTreeMap<String, Ty>,
) -> Result<LoweredFunction, FunctionLoweringFailure> {
    let node = NodeId::owner_local(definition, 0);
    let mut initial_source_plan = vec![
        (SourceRef::definition(definition), header.name.source),
        (SourceRef::node(node), header.name.source),
    ];
    let Some(body) = body.block.as_ref() else {
        return Err(FunctionLoweringFailure {
            diagnostic: Diagnostic::unsupported(
                "bodyless declarations require an explicit runtime intrinsic",
                SourceRef::definition(definition),
            ),
            source_plan: initial_source_plan,
        });
    };
    if let Err(error) = goto_scopes::validate(body) {
        let source = SourceRef::node(NodeId::owner_local(definition, 1));
        initial_source_plan.push((source, error.source));
        return Err(FunctionLoweringFailure {
            diagnostic: Diagnostic::semantic(error.message, source),
            source_plan: initial_source_plan,
        });
    }
    let mut lowerer = FunctionLowerer {
        owner: definition,
        next_node: 1,
        next_local_type: 0,
        next_control_target: 0,
        functions: symbols.functions,
        qualified_functions: symbols.qualified_functions,
        methods: symbols.methods,
        generic_functions: symbols.generic_functions,
        generic_methods: symbols.generic_methods,
        generic_types: symbols.generic_types,
        constants: symbols.constants,
        qualified_constants: symbols.qualified_constants,
        local_constant_scopes: vec![BTreeMap::new()],
        variables: symbols.variables,
        qualified_variables: symbols.qualified_variables,
        package_imports: symbols.package_imports,
        intrinsic_packages: symbols.intrinsic_packages,
        type_aliases,
        type_scope_changes: vec![BTreeMap::new()],
        signature: signature.clone(),
        locals: Vec::new(),
        scopes: vec![BTreeMap::new()],
        closures: Vec::new(),
        closure_scopes: vec![BTreeMap::new()],
        named_results: Vec::new(),
        control_targets: Vec::new(),
        range_yield_target: None,
        iteration_capture_scopes: Vec::new(),
        declared_labels: BTreeSet::new(),
        referenced_gotos: BTreeMap::new(),
        defer_registration_depth: 0,
        inside_deferred_closure: false,
        inside_local_closure: false,
        active_generic_functions: Vec::new(),
        source_override: None,
        source_plan: initial_source_plan,
    };
    let lowered = (|| {
        let receiver_count = usize::from(header.receiver.is_some());
        let (receiver_types, parameter_types) =
            signature
                .params
                .split_at_checked(receiver_count)
                .ok_or_else(|| Diagnostic::backend("method signature omitted its receiver type"))?;
        let mut params = Vec::new();
        if let Some(receiver) = &header.receiver {
            params.extend(lowerer.declare_field_bindings(
                receiver,
                receiver_types,
                hir::LocalKind::Parameter,
            )?);
        }
        params.extend(lowerer.declare_field_bindings(
            &header.params,
            parameter_types,
            hir::LocalKind::Parameter,
        )?);
        lowerer.named_results = header.results.as_ref().map_or_else(
            || Ok(Vec::new()),
            |results| lowerer.declare_result_bindings(results, &signature.results),
        )?;
        let body = lowerer.lower_block(body, false)?;
        if let Some((label, source)) = lowerer
            .referenced_gotos
            .iter()
            .find(|(label, _)| !lowerer.declared_labels.contains(*label))
        {
            return Err(Diagnostic::semantic(
                format!("goto target {label} is not defined"),
                *source,
            ));
        }
        Ok((params, body))
    })();
    match lowered {
        Ok((params, body)) => Ok(LoweredFunction {
            function: hir::Function {
                id: definition,
                node,
                name: header.name.name.to_string(),
                signature,
                params,
                named_results: lowerer.named_results,
                locals: lowerer.locals,
                closures: lowerer.closures,
                body,
                source: SourceRef::definition(definition),
            },
            source_plan: lowerer.source_plan,
        }),
        Err(diagnostic) => Err(FunctionLoweringFailure {
            diagnostic,
            source_plan: lowerer.source_plan,
        }),
    }
}
