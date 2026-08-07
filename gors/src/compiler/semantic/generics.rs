//! Type-checked instantiation of generic declarations into caller-owned HIR.

mod constraints;

use std::collections::{BTreeMap, BTreeSet};

use super::expressions::coerce_expr;
use super::{FunctionLowerer, GenericFunctionSymbol, GenericTypeSymbol, lower_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{ClosureId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, FunctionHeaderSyntax, SyntaxSource,
};
use crate::compiler::types::{Signature, Ty, UntypedTy};
use crate::token::Token;

type LoweredGenericBody = (
    Vec<crate::compiler::ids::LocalId>,
    Vec<Option<crate::compiler::ids::LocalId>>,
    hir::Block,
);
use constraints::{type_parameter_names, validate_constraints, validate_declared_constraints};

impl FunctionLowerer {
    pub(super) fn lower_semantic_type(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<Ty, Diagnostic> {
        lower_type_with_generics(expression, &self.type_aliases, &self.generic_types, source)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_generic_function_call(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let symbol = self
            .generic_functions
            .get(name)
            .cloned()
            .ok_or_else(|| Diagnostic::backend(format!("missing generic function {name}")))?;
        if spread {
            return Err(Diagnostic::unsupported(
                "generic variadic calls are not yet implemented",
                source,
            ));
        }
        let mut args = arguments
            .iter()
            .map(|argument| self.lower_expr(argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        let substitutions = infer_function_arguments(
            &symbol.header,
            &args,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        validate_declared_constraints(
            symbol.header.type_parameters.as_ref(),
            &substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let signature = instantiate_signature(
            &symbol.header,
            &substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        coerce_arguments(&mut args, &signature.params, source, "generic function")?;
        let closure = self.lower_instantiated_generic_closure(
            &symbol,
            signature.clone(),
            &substitutions,
            syntax_source,
            source,
        )?;
        self.finish_generic_call(
            closure,
            args,
            signature.results,
            node,
            source,
            expected,
            allow_discarded_call_result,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_lower_generic_method_call(
        &mut self,
        mut receiver: hir::Expr,
        member: &str,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<Option<hir::Expr>, Diagnostic> {
        let Some(definition) = named_receiver_definition(&receiver.ty) else {
            return Ok(None);
        };
        let Some(symbol) = self
            .generic_methods
            .get(&(definition, member.to_owned()))
            .cloned()
        else {
            return Ok(None);
        };
        if spread {
            return Err(Diagnostic::unsupported(
                "generic variadic method calls are not yet implemented",
                source,
            ));
        }
        let receiver_syntax = single_receiver_type(&symbol.header, source)?;
        let generic_type = self
            .generic_types
            .values()
            .find(|generic| generic.id == definition)
            .cloned()
            .ok_or_else(|| Diagnostic::backend("generic method receiver type disappeared"))?;
        let mut substitutions = BTreeMap::new();
        let receiver_parameters = type_parameter_names(&generic_type.type_parameters, source)?;
        infer_type_expression(
            receiver_syntax,
            &receiver.ty,
            &receiver_parameters,
            &mut substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let mut ordinary_args = arguments
            .iter()
            .map(|argument| self.lower_expr(argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        infer_parameter_list(
            &symbol.header.params,
            &ordinary_args,
            &receiver_parameters,
            &mut substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        validate_constraints(
            &generic_type.type_parameters,
            &substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let signature = instantiate_signature(
            &symbol.header,
            &substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let Some((receiver_ty, parameters)) = signature.params.split_first() else {
            return Err(Diagnostic::backend(
                "generic method signature omitted its receiver",
            ));
        };
        receiver = self.adjust_method_receiver(
            receiver,
            receiver_ty,
            symbol.pointer_receiver,
            syntax_source,
            source,
        )?;
        coerce_arguments(&mut ordinary_args, parameters, source, "generic method")?;
        let mut args = Vec::with_capacity(ordinary_args.len().saturating_add(1));
        args.push(receiver);
        args.extend(ordinary_args);
        let closure = self.lower_instantiated_generic_closure(
            &symbol,
            signature.clone(),
            &substitutions,
            syntax_source,
            source,
        )?;
        self.finish_generic_call(
            closure,
            args,
            signature.results,
            node,
            source,
            expected,
            allow_discarded_call_result,
        )
        .map(Some)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_generic_call(
        &mut self,
        closure: ClosureId,
        args: Vec<hir::Expr>,
        results: Vec<Ty>,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let ty = match results.as_slice() {
            [] => Ty::Unit,
            [single] => single.clone(),
            many => Ty::Tuple(many.to_vec()),
        };
        if ty == Ty::Unit && !allow_discarded_call_result {
            return Err(Diagnostic::unsupported(
                "a no-result generic call cannot be used as a value",
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
                may_read: false,
            },
            |effects, argument| effects.union(argument.effects),
        );
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Closure(closure),
                args,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    fn lower_instantiated_generic_closure(
        &mut self,
        symbol: &GenericFunctionSymbol,
        signature: Signature,
        substitutions: &BTreeMap<String, Ty>,
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<ClosureId, Diagnostic> {
        if self.active_generic_functions.contains(&symbol.id) {
            return Err(Diagnostic::unsupported(
                "recursive generic instantiation is not yet implemented",
                source,
            ));
        }
        let body = symbol.body.block.as_ref().ok_or_else(|| {
            Diagnostic::unsupported("generic function declaration has no body", source)
        })?;
        let id = ClosureId(
            u32::try_from(self.closures.len())
                .map_err(|_| Diagnostic::backend("function exceeds the local closure ID space"))?,
        );
        let placeholder_node = self.alloc_node(syntax_source)?;
        self.closures.push(hir::Closure {
            id,
            signature: signature.clone(),
            params: Vec::new(),
            named_results: Vec::new(),
            body: hir::Block {
                node: placeholder_node,
                stmts: Vec::new(),
                source: SourceRef::node(placeholder_node),
            },
            source,
        });

        let previous_aliases = self.type_aliases.clone();
        self.type_aliases.extend(substitutions.clone());
        let previous_override = self.source_override.replace(syntax_source);
        self.push_scope();
        let previous_signature = std::mem::replace(&mut self.signature, signature.clone());
        let previous_named_results = std::mem::take(&mut self.named_results);
        let previous_loops = std::mem::take(&mut self.loop_labels);
        let previous_labels = std::mem::take(&mut self.declared_labels);
        let previous_gotos = std::mem::take(&mut self.referenced_gotos);
        let previous_inside = self.inside_local_closure;
        self.inside_local_closure = true;
        self.active_generic_functions.push(symbol.id);

        let lowered = self.lower_generic_body(&symbol.header, body, &signature);

        self.active_generic_functions.pop();
        self.inside_local_closure = previous_inside;
        self.signature = previous_signature;
        self.named_results = previous_named_results;
        self.loop_labels = previous_loops;
        self.declared_labels = previous_labels;
        self.referenced_gotos = previous_gotos;
        self.pop_scope();
        self.source_override = previous_override;
        self.type_aliases = previous_aliases;

        let (params, named_results, body) = lowered?;
        let closure = self
            .closures
            .get_mut(id.index() as usize)
            .ok_or_else(|| Diagnostic::backend("reserved generic closure disappeared"))?;
        *closure = hir::Closure {
            id,
            signature,
            params,
            named_results,
            body,
            source,
        };
        Ok(id)
    }

    fn lower_generic_body(
        &mut self,
        header: &FunctionHeaderSyntax,
        body: &BlockSyntax,
        signature: &Signature,
    ) -> Result<LoweredGenericBody, Diagnostic> {
        let receiver_count = usize::from(header.receiver.is_some());
        let (receiver_types, parameter_types) = signature
            .params
            .split_at_checked(receiver_count)
            .ok_or_else(|| {
            Diagnostic::backend("generic method signature omitted its receiver")
        })?;
        let mut params = Vec::new();
        if let Some(receiver) = &header.receiver {
            params.extend(self.declare_field_bindings(
                receiver,
                receiver_types,
                hir::LocalKind::Parameter,
            )?);
        }
        params.extend(self.declare_field_bindings(
            &header.params,
            parameter_types,
            hir::LocalKind::Parameter,
        )?);
        let named_results = header.results.as_ref().map_or_else(
            || Ok(Vec::new()),
            |results| self.declare_result_bindings(results, &signature.results),
        )?;
        self.named_results = named_results.clone();
        let body = self.lower_block(body, false)?;
        Ok((params, named_results, body))
    }
}

pub(super) fn lower_type_with_generics(
    expression: &ExprSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    let (base, arguments) = match &expression.kind {
        ExprSyntaxKind::Index { base, index } => {
            (base.as_ref(), std::slice::from_ref(index.as_ref()))
        }
        ExprSyntaxKind::IndexList { base, indices } => (base.as_ref(), indices.as_ref()),
        _ => return lower_type(expression, aliases, source),
    };
    let ExprSyntaxKind::Ident(base) = &base.kind else {
        return Err(Diagnostic::unsupported(
            "parameterized type base must be a named type",
            source,
        ));
    };
    let generic = generic_types.get(base.name.as_ref()).ok_or_else(|| {
        Diagnostic::semantic(format!("{} is not a generic type", base.name), source)
    })?;
    let parameters = type_parameter_names(&generic.type_parameters, source)?;
    let parameters = parameters.into_iter().collect::<Vec<_>>();
    if parameters.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic type {} requires {} type arguments; got {}",
                base.name,
                parameters.len(),
                arguments.len()
            ),
            source,
        ));
    }
    let substitutions = parameters
        .into_iter()
        .zip(arguments)
        .map(|(parameter, argument)| {
            lower_type_with_generics(argument, aliases, generic_types, source)
                .map(|argument| (parameter, argument))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    validate_constraints(
        &generic.type_parameters,
        &substitutions,
        aliases,
        generic_types,
        source,
    )?;
    let mut instantiated_aliases = aliases.clone();
    instantiated_aliases.extend(substitutions);
    let underlying = lower_type_with_generics(
        &generic.underlying,
        &instantiated_aliases,
        generic_types,
        source,
    )?;
    Ok(Ty::Named {
        definition: generic.id,
        underlying: Box::new(underlying.underlying().clone()),
    })
}

fn infer_function_arguments(
    header: &FunctionHeaderSyntax,
    arguments: &[hir::Expr],
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<BTreeMap<String, Ty>, Diagnostic> {
    let parameters = header
        .type_parameters
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic function omitted its type-parameter syntax"))?;
    let names = type_parameter_names(parameters, source)?;
    let mut substitutions = BTreeMap::new();
    infer_parameter_list(
        &header.params,
        arguments,
        &names,
        &mut substitutions,
        aliases,
        generic_types,
        source,
    )?;
    let missing = names
        .iter()
        .filter(|name| !substitutions.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::semantic(
            format!("cannot infer type parameters {}", missing.join(", ")),
            source,
        ));
    }
    Ok(substitutions)
}

#[allow(clippy::too_many_arguments)]
fn infer_parameter_list(
    fields: &FieldListSyntax,
    arguments: &[hir::Expr],
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let formal = repeated_field_types(fields, source)?;
    if formal.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic call has {} arguments; expected {}",
                arguments.len(),
                formal.len()
            ),
            source,
        ));
    }
    // Typed arguments determine type parameters first. Untyped constant
    // arguments adapt to a parameter a typed argument already determined
    // (argument coercion checks representability); only parameters no typed
    // argument determined receive the merged default type of their untyped
    // constant arguments.
    for (formal, actual) in formal.iter().zip(arguments) {
        if matches!(actual.ty, Ty::Untyped(_)) {
            continue;
        }
        infer_type_expression(
            formal,
            &actual.ty.default_typed(),
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        )?;
    }
    let mut constant_defaults = BTreeMap::<String, UntypedTy>::new();
    for (formal, actual) in formal.iter().zip(arguments) {
        let Ty::Untyped(kind) = actual.ty else {
            continue;
        };
        let Some(parameter) = bare_type_parameter(formal, parameter_names) else {
            infer_type_expression(
                formal,
                &actual.ty.default_typed(),
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )?;
            continue;
        };
        if substitutions.contains_key(parameter) {
            continue;
        }
        let merged = match constant_defaults.get(parameter) {
            None => kind,
            Some(previous) => merge_untyped_constant_kinds(*previous, kind).ok_or_else(|| {
                Diagnostic::semantic(
                    format!(
                        "mismatched default types {:?} and {:?} for {parameter}",
                        Ty::Untyped(*previous).default_typed(),
                        Ty::Untyped(kind).default_typed()
                    ),
                    source,
                )
            })?,
        };
        constant_defaults.insert(parameter.to_string(), merged);
    }
    for (parameter, kind) in constant_defaults {
        substitutions.insert(parameter, Ty::Untyped(kind).default_typed());
    }
    Ok(())
}

/// The type-parameter name a formal parameter references directly, if any.
fn bare_type_parameter<'syntax>(
    formal: &'syntax ExprSyntax,
    parameter_names: &BTreeSet<String>,
) -> Option<&'syntax str> {
    match &formal.kind {
        ExprSyntaxKind::Paren(inner) => bare_type_parameter(inner, parameter_names),
        ExprSyntaxKind::Ident(ident) if parameter_names.contains(ident.name.as_ref()) => {
            Some(ident.name.as_ref())
        }
        _ => None,
    }
}

/// Merged default-type kind of two untyped constants inferred for one type
/// parameter. Numeric kinds merge to the kind appearing later in the
/// specification order integer, floating-point, complex; other kinds only
/// merge with themselves.
fn merge_untyped_constant_kinds(previous: UntypedTy, next: UntypedTy) -> Option<UntypedTy> {
    if previous == next {
        return Some(previous);
    }
    let rank = |kind: UntypedTy| match kind {
        UntypedTy::Int => Some(0_u8),
        UntypedTy::Float => Some(1),
        UntypedTy::Complex => Some(2),
        UntypedTy::Bool | UntypedTy::String => None,
    };
    let previous_rank = rank(previous)?;
    let next_rank = rank(next)?;
    Some(if previous_rank >= next_rank {
        previous
    } else {
        next
    })
}

#[allow(clippy::too_many_arguments)]
fn infer_type_expression(
    formal: &ExprSyntax,
    actual: &Ty,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    match &formal.kind {
        ExprSyntaxKind::Paren(formal) => infer_type_expression(
            formal,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::Ident(ident) if parameter_names.contains(ident.name.as_ref()) => {
            let actual = actual.default_typed();
            if let Some(previous) = substitutions.get(ident.name.as_ref())
                && previous != &actual
            {
                return Err(Diagnostic::semantic(
                    format!(
                        "conflicting inferred types for {}: {previous:?} and {actual:?}",
                        ident.name
                    ),
                    source,
                ));
            }
            substitutions.insert(ident.name.to_string(), actual);
            Ok(())
        }
        ExprSyntaxKind::ArrayType {
            length: None,
            element,
        } => {
            let Ty::Slice(actual) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            infer_type_expression(
                element,
                actual,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::Unary {
            token: Token::MUL,
            expression,
        } => {
            let Ty::Pointer(actual) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            infer_type_expression(
                expression,
                actual,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::Index { base, .. } | ExprSyntaxKind::IndexList { base, .. } => {
            let ExprSyntaxKind::Ident(base) = &base.kind else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            let generic = generic_types.get(base.name.as_ref()).ok_or_else(|| {
                Diagnostic::semantic(format!("{} is not a generic type", base.name), source)
            })?;
            let Ty::Named {
                definition,
                underlying,
            } = actual
            else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            if *definition != generic.id {
                return Err(type_inference_mismatch(formal, actual, source));
            }
            infer_type_expression(
                &generic.underlying,
                underlying,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::StructType { fields } => {
            let Ty::Struct(actual_fields) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            if fields.fields.len() != actual_fields.len() {
                return Err(type_inference_mismatch(formal, actual, source));
            }
            for (formal, actual) in fields.fields.iter().zip(actual_fields) {
                let formal = formal
                    .ty
                    .as_ref()
                    .ok_or_else(|| Diagnostic::backend("generic struct field omitted its type"))?;
                infer_type_expression(
                    formal,
                    &actual.ty,
                    parameter_names,
                    substitutions,
                    aliases,
                    generic_types,
                    source,
                )?;
            }
            Ok(())
        }
        _ => {
            let mut instantiated_aliases = aliases.clone();
            instantiated_aliases.extend(substitutions.clone());
            let expected =
                lower_type_with_generics(formal, &instantiated_aliases, generic_types, source)?;
            if expected == *actual {
                Ok(())
            } else {
                Err(type_inference_mismatch(formal, actual, source))
            }
        }
    }
}

fn type_inference_mismatch(_formal: &ExprSyntax, actual: &Ty, source: SourceRef) -> Diagnostic {
    Diagnostic::semantic(
        format!("argument type {actual:?} does not match the generic parameter pattern"),
        source,
    )
}

fn instantiate_signature(
    header: &FunctionHeaderSyntax,
    substitutions: &BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Signature, Diagnostic> {
    let mut aliases = aliases.clone();
    aliases.extend(substitutions.clone());
    let mut params = Vec::new();
    if let Some(receiver) = &header.receiver {
        let (receiver, variadic) =
            parameter_types_with_generics(receiver, &aliases, generic_types, source)?;
        if variadic || receiver.len() != 1 {
            return Err(Diagnostic::semantic(
                "a method must declare exactly one non-variadic receiver",
                source,
            ));
        }
        params.extend(receiver);
    }
    let (ordinary, variadic) =
        parameter_types_with_generics(&header.params, &aliases, generic_types, source)?;
    params.extend(ordinary);
    let results = header
        .results
        .as_ref()
        .map(|results| field_types_with_generics(results, &aliases, generic_types, source))
        .transpose()?
        .unwrap_or_default();
    Ok(Signature {
        params,
        results,
        variadic,
    })
}

fn field_types_with_generics(
    fields: &FieldListSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Vec<Ty>, Diagnostic> {
    let mut result = Vec::new();
    for field in &*fields.fields {
        if field.variadic {
            return Err(Diagnostic::semantic(
                "result parameters cannot be variadic",
                source,
            ));
        }
        let expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let ty = lower_type_with_generics(expression, aliases, generic_types, source)?;
        result.extend(std::iter::repeat_n(
            ty,
            field.names.as_ref().map_or(1, |names| names.len()),
        ));
    }
    Ok(result)
}

fn parameter_types_with_generics(
    fields: &FieldListSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = false;
    for (index, field) in fields.fields.iter().enumerate() {
        let expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let mut ty = lower_type_with_generics(expression, aliases, generic_types, source)?;
        let count = field.names.as_ref().map_or(1, |names| names.len());
        if field.variadic {
            if variadic || index + 1 != fields.fields.len() || count != 1 {
                return Err(Diagnostic::semantic(
                    "a variadic parameter must be the final single parameter",
                    source,
                ));
            }
            variadic = true;
            ty = Ty::Slice(Box::new(ty));
        }
        result.extend(std::iter::repeat_n(ty, count));
    }
    Ok((result, variadic))
}

fn repeated_field_types(
    fields: &FieldListSyntax,
    source: SourceRef,
) -> Result<Vec<&ExprSyntax>, Diagnostic> {
    let mut result = Vec::new();
    for field in &*fields.fields {
        if field.variadic {
            return Err(Diagnostic::unsupported(
                "generic variadic functions are not yet implemented",
                source,
            ));
        }
        let ty = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("generic parameter omitted its type"))?;
        result.extend(std::iter::repeat_n(
            ty,
            field.names.as_ref().map_or(1, |names| names.len()),
        ));
    }
    Ok(result)
}

fn coerce_arguments(
    arguments: &mut [hir::Expr],
    parameters: &[Ty],
    source: SourceRef,
    callable: &str,
) -> Result<(), Diagnostic> {
    if arguments.len() != parameters.len() {
        return Err(Diagnostic::semantic(
            format!(
                "{callable} call has {} arguments; expected {}",
                arguments.len(),
                parameters.len()
            ),
            source,
        ));
    }
    for (argument, parameter) in arguments.iter_mut().zip(parameters) {
        coerce_expr(argument, parameter, source)?;
    }
    Ok(())
}

fn single_receiver_type(
    header: &FunctionHeaderSyntax,
    source: SourceRef,
) -> Result<&ExprSyntax, Diagnostic> {
    let receiver = header
        .receiver
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic method omitted its receiver"))?;
    let [field] = receiver.fields.as_ref() else {
        return Err(Diagnostic::semantic(
            "a method must declare exactly one receiver",
            source,
        ));
    };
    field
        .ty
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic method receiver omitted its type"))
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
