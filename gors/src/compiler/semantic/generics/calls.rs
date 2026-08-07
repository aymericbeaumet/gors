//! Generic function and method call instantiation.

use super::*;
use crate::compiler::semantic::calls::{LoweredCallArguments, forwarded_call_result_types};

fn flattened_call_argument_types(arguments: &[hir::Expr]) -> Vec<Ty> {
    if let [argument] = arguments
        && let Some(results) = forwarded_call_result_types(argument)
    {
        return results.to_vec();
    }
    arguments
        .iter()
        .map(|argument| argument.ty.clone())
        .collect()
}

impl FunctionLowerer {
    pub(in crate::compiler::semantic) fn lower_semantic_type(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<Ty, Diagnostic> {
        lower_type_with_generics(expression, &self.type_aliases, &self.generic_types, source)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::compiler::semantic) fn lower_generic_function_call(
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
        self.lower_explicit_generic_function_call(
            name,
            &[],
            arguments,
            spread,
            node,
            syntax_source,
            source,
            expected,
            allow_discarded_call_result,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::compiler::semantic) fn lower_explicit_generic_function_call(
        &mut self,
        name: &str,
        type_arguments: &[ExprSyntax],
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
        let inference_args = arguments
            .iter()
            .map(|argument| self.lower_expr(argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        let inference_types = flattened_call_argument_types(&inference_args);
        let substitutions = infer_function_arguments(
            &symbol.header,
            type_arguments,
            &inference_types,
            spread,
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
            None,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let args = self.coerce_lowered_generic_arguments(
            inference_args,
            arguments,
            &signature.params,
            signature.variadic,
            spread,
            syntax_source,
            source,
            "generic function",
        )?;
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
            Vec::new(),
            signature.results,
            node,
            source,
            expected,
            allow_discarded_call_result,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn coerce_lowered_generic_arguments(
        &mut self,
        mut arguments: Vec<hir::Expr>,
        syntax: &[ExprSyntax],
        parameters: &[Ty],
        variadic: bool,
        spread: bool,
        pack_source: SyntaxSource,
        source: SourceRef,
        callable: &str,
    ) -> Result<LoweredCallArguments, Diagnostic> {
        if arguments.len() != syntax.len() {
            return Err(Diagnostic::backend(
                "generic argument syntax and lowered values have different lengths",
            ));
        }
        if spread && !variadic {
            return Err(Diagnostic::semantic(
                format!("... is only valid when calling a variadic {callable}"),
                source,
            ));
        }
        if let [argument] = arguments.as_slice()
            && let Some(results) = forwarded_call_result_types(argument)
        {
            if spread {
                return Err(Diagnostic::semantic(
                    format!("cannot use ... with {}-valued function call", results.len()),
                    source,
                ));
            }
            return self.plan_forwarded_call_arguments(
                argument.clone(),
                results.to_vec(),
                parameters,
                variadic,
                source,
                callable,
            );
        }
        if !variadic || spread {
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
            for ((argument, syntax), parameter) in arguments.iter_mut().zip(syntax).zip(parameters)
            {
                self.coerce_lowered_generic_argument(argument, parameter, syntax.source)?;
            }
            return Ok(LoweredCallArguments::Explicit(arguments));
        }

        let Some((variadic_parameter, fixed_parameters)) = parameters.split_last() else {
            return Err(Diagnostic::backend(
                "variadic generic signature has no final slice parameter",
            ));
        };
        if arguments.len() < fixed_parameters.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "{callable} call has {} arguments; requires at least {}",
                    arguments.len(),
                    fixed_parameters.len()
                ),
                source,
            ));
        }
        let mut variadic_arguments = arguments.split_off(fixed_parameters.len());
        let Some((fixed_syntax, variadic_syntax)) = syntax.split_at_checked(fixed_parameters.len())
        else {
            return Err(Diagnostic::backend(
                "generic argument syntax is shorter than the fixed parameter list",
            ));
        };
        for ((argument, syntax), parameter) in
            arguments.iter_mut().zip(fixed_syntax).zip(fixed_parameters)
        {
            self.coerce_lowered_generic_argument(argument, parameter, syntax.source)?;
        }

        let Ty::Slice(element) = variadic_parameter.underlying() else {
            return Err(Diagnostic::backend(
                "variadic generic signature final parameter is not a slice",
            ));
        };
        if variadic_arguments.is_empty() {
            let node = self.alloc_node(pack_source)?;
            arguments.push(self.zero_value_expr(
                node,
                SourceRef::node(node),
                variadic_parameter.clone(),
            )?);
            return Ok(LoweredCallArguments::Explicit(arguments));
        }
        if element.underlying() != &Ty::Int(crate::compiler::types::IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "generic variadic slice packing currently supports int elements",
                source,
            ));
        }
        for (argument, syntax) in variadic_arguments.iter_mut().zip(variadic_syntax) {
            self.coerce_lowered_generic_argument(argument, element, syntax.source)?;
        }
        let effects = variadic_arguments
            .iter()
            .fold(hir::Effects::default(), |effects, value| {
                effects.union(value.effects)
            })
            .union(hir::Effects {
                may_call: true,
                may_allocate: true,
                may_write: true,
                may_panic: true,
                ..hir::Effects::default()
            });
        let node = self.alloc_node(pack_source)?;
        arguments.push(hir::Expr {
            node,
            kind: hir::ExprKind::DynamicSliceLiteralI64(variadic_arguments),
            ty: variadic_parameter.clone(),
            category: hir::ValueCategory::Value,
            effects,
            source: SourceRef::node(node),
        });
        Ok(LoweredCallArguments::Explicit(arguments))
    }

    fn coerce_lowered_generic_argument(
        &mut self,
        argument: &mut hir::Expr,
        parameter: &Ty,
        syntax_source: SyntaxSource,
    ) -> Result<(), Diagnostic> {
        let source = argument.source;
        if matches!(parameter.underlying(), Ty::Interface(_)) {
            *argument = self.coerce_interface_value(argument.clone(), parameter, syntax_source)?;
        } else {
            coerce_expr(argument, parameter, source)?;
        }
        Ok(())
    }

    pub(in crate::compiler::semantic) fn lower_generic_function_binding(
        &mut self,
        binding: &IdentSyntax,
        name: &str,
        type_arguments: &[ExprSyntax],
        syntax_source: SyntaxSource,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if binding.name.as_ref() == "_" {
            return Err(Diagnostic::semantic(
                "a generic function binding cannot use the blank identifier",
                source,
            ));
        }
        let symbol = self
            .generic_functions
            .get(name)
            .cloned()
            .ok_or_else(|| Diagnostic::backend(format!("missing generic function {name}")))?;
        let parameters = symbol.header.type_parameters.as_ref().ok_or_else(|| {
            Diagnostic::backend("generic function omitted its type-parameter syntax")
        })?;
        let ordered_names = type_parameter_names_in_order(parameters, source)?;
        if type_arguments.len() > ordered_names.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "generic function requires at most {} type arguments; got {}",
                    ordered_names.len(),
                    type_arguments.len()
                ),
                source,
            ));
        }
        let names = ordered_names.iter().cloned().collect::<BTreeSet<_>>();
        let mut substitutions = ordered_names
            .iter()
            .zip(type_arguments)
            .map(|(parameter, argument)| {
                lower_type_with_generics(argument, &self.type_aliases, &self.generic_types, source)
                    .map(|argument| (parameter.clone(), argument))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        infer_constraint_arguments(
            parameters,
            &names,
            &mut substitutions,
            &self.type_aliases,
            &self.generic_types,
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
            None,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let closure = self.lower_instantiated_generic_closure(
            &symbol,
            signature,
            &substitutions,
            syntax_source,
            source,
        )?;
        self.bind_closure(&binding.name, closure, source)?;
        Ok(hir::StmtKind::ClosureBinding(closure))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::compiler::semantic) fn try_lower_generic_method_call(
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
        let receiver_aliases = infer_receiver_parameter_aliases(
            receiver_syntax,
            &generic_type.type_parameters,
            &substitutions,
            source,
        )?;
        let mut body_substitutions = substitutions.clone();
        body_substitutions.extend(receiver_aliases);
        let method_parameter_names = body_substitutions.keys().cloned().collect::<BTreeSet<_>>();
        let ordinary_args = arguments
            .iter()
            .map(|argument| self.lower_expr(argument, None))
            .collect::<Result<Vec<_>, _>>()?;
        let inference_types = flattened_call_argument_types(&ordinary_args);
        infer_parameter_list(
            &symbol.header.params,
            &inference_types,
            spread,
            &method_parameter_names,
            &mut body_substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        validate_constraints(
            &generic_type.type_parameters,
            &body_substitutions,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let receiver_ty = instantiate_generic_receiver(
            &generic_type,
            &substitutions,
            symbol.pointer_receiver,
            &self.type_aliases,
            &self.generic_types,
            source,
        )?;
        let signature = instantiate_signature(
            &symbol.header,
            &body_substitutions,
            Some(receiver_ty),
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
        let args = self.coerce_lowered_generic_arguments(
            ordinary_args,
            arguments,
            parameters,
            signature.variadic,
            spread,
            syntax_source,
            source,
            "generic method",
        )?;
        let closure = self.lower_instantiated_generic_closure(
            &symbol,
            signature.clone(),
            &body_substitutions,
            syntax_source,
            source,
        )?;
        self.finish_generic_call(
            closure,
            args,
            vec![receiver],
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
        args: LoweredCallArguments,
        prefix: Vec<hir::Expr>,
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
        let effects = prefix
            .iter()
            .fold(
                hir::Effects {
                    may_call: true,
                    may_allocate: true,
                    may_block: true,
                    may_panic: true,
                    may_write: true,
                    may_read: false,
                },
                |effects, argument| effects.union(argument.effects),
            )
            .union(args.effects());
        let mut result = hir::Expr {
            node,
            kind: args.into_call_kind(hir::Callee::Closure(closure), prefix),
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
        let previous_targets = std::mem::take(&mut self.control_targets);
        let previous_range_yield_target = self.range_yield_target.take();
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
        self.control_targets = previous_targets;
        self.range_yield_target = previous_range_yield_target;
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
