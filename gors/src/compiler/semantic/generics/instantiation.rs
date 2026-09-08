//! Exact generic type and signature instantiation.

mod recursion;

use std::collections::BTreeMap;

use super::constraints::{type_parameter_names_in_order, validate_constraints};
use super::{GenericTypeSymbol, MethodEnvironment};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ChannelDirectionSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, FunctionHeaderSyntax,
};
use crate::compiler::types::{
    ChannelDir, ConstValue, InterfaceMethod, NamedTypeId, Signature, StructField, Ty,
};
use crate::token::Token;

pub(in crate::compiler::semantic) fn lower_type_with_generics(
    expression: &ExprSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    lower_type_with_generic_constant_lookup(
        expression,
        aliases,
        generic_types,
        methods,
        &|_| None,
        source,
    )
}

pub(in crate::compiler::semantic) fn lower_type_with_generic_constant_lookup(
    expression: &ExprSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    InstantiationContext {
        generic_types,
        methods,
        constant_lookup,
        source,
        active: Vec::new(),
    }
    .lower(expression, aliases, 0)
}

#[derive(Clone)]
struct ActiveInstantiation {
    identity: NamedTypeId,
    guard_depth: usize,
    alias: bool,
}

struct InstantiationContext<'symbols, 'constants, Lookup>
where
    Lookup: Fn(&str) -> Option<(Ty, ConstValue)>,
{
    generic_types: &'symbols BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'symbols>,
    constant_lookup: &'constants Lookup,
    source: SourceRef,
    active: Vec<ActiveInstantiation>,
}

impl<Lookup> InstantiationContext<'_, '_, Lookup>
where
    Lookup: Fn(&str) -> Option<(Ty, ConstValue)>,
{
    fn lower(
        &mut self,
        expression: &ExprSyntax,
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<Ty, Diagnostic> {
        match &expression.kind {
            ExprSyntaxKind::Paren(inner) => self.lower(inner, aliases, guard_depth),
            ExprSyntaxKind::ArrayType { length, element } => {
                let nested_depth = if length.is_none() {
                    guard_depth.saturating_add(1)
                } else {
                    guard_depth
                };
                let element = self.lower(element, aliases, nested_depth)?;
                let Some(length) = length else {
                    return Ok(Ty::Slice(Box::new(element)));
                };
                let (length_ty, length_value) = super::super::eval_constant_with_lookup(
                    length,
                    self.constant_lookup,
                    self.source,
                    None,
                )?;
                let length = super::super::array_length_from_constant(
                    &length_ty,
                    &length_value,
                    self.source,
                )?;
                Ok(Ty::Array(length, Box::new(element)))
            }
            ExprSyntaxKind::MapType { key, value } => {
                let nested_depth = guard_depth.saturating_add(1);
                Ok(Ty::Map(
                    Box::new(self.lower(key, aliases, nested_depth)?),
                    Box::new(self.lower(value, aliases, nested_depth)?),
                ))
            }
            ExprSyntaxKind::ChannelType { direction, element } => {
                let direction = match direction {
                    ChannelDirectionSyntax::SendReceive => ChannelDir::SendReceive,
                    ChannelDirectionSyntax::SendOnly => ChannelDir::SendOnly,
                    ChannelDirectionSyntax::ReceiveOnly => ChannelDir::ReceiveOnly,
                };
                Ok(Ty::Channel(
                    direction,
                    Box::new(self.lower(element, aliases, guard_depth.saturating_add(1))?),
                ))
            }
            ExprSyntaxKind::Unary {
                token: Token::MUL,
                expression,
            } => Ok(Ty::Pointer(Box::new(self.lower(
                expression,
                aliases,
                guard_depth.saturating_add(1),
            )?))),
            ExprSyntaxKind::FunctionType {
                has_type_parameters,
                params,
                results,
            } => {
                if *has_type_parameters {
                    return Err(Diagnostic::unsupported(
                        "generic function types are not yet implemented",
                        self.source,
                    ));
                }
                let nested_depth = guard_depth.saturating_add(1);
                let (params, variadic) = self.parameter_types(params, aliases, nested_depth)?;
                let results = results
                    .as_ref()
                    .map(|results| self.field_types(results, aliases, nested_depth))
                    .transpose()?
                    .unwrap_or_default();
                Ok(Ty::Function(Signature {
                    params,
                    results,
                    variadic,
                }))
            }
            ExprSyntaxKind::StructType { fields } => {
                self.lower_struct(fields, aliases, guard_depth)
            }
            ExprSyntaxKind::InterfaceType { methods } => {
                self.lower_interface(methods, aliases, guard_depth)
            }
            ExprSyntaxKind::Index { base, index } => self.instantiate(
                base,
                std::slice::from_ref(index.as_ref()),
                aliases,
                guard_depth,
            ),
            ExprSyntaxKind::IndexList { base, indices } => {
                self.instantiate(base, indices, aliases, guard_depth)
            }
            _ => super::super::lower_type_with_constant_lookup(
                expression,
                aliases,
                self.constant_lookup,
                self.source,
            ),
        }
    }

    fn lower_struct(
        &mut self,
        fields: &FieldListSyntax,
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<Ty, Diagnostic> {
        let mut lowered = Vec::new();
        for field in &*fields.fields {
            if field.variadic {
                return Err(Diagnostic::semantic(
                    "struct fields cannot be variadic",
                    self.source,
                ));
            }
            let syntax = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("struct field has no type"))?;
            let ty = self.lower(syntax, aliases, guard_depth)?;
            let tag = field.tag.as_ref().map(ToString::to_string);
            if let Some(names) = &field.names {
                lowered.extend(names.iter().map(|name| StructField {
                    name: name.name.to_string(),
                    ty: ty.clone(),
                    embedded: false,
                    tag: tag.clone(),
                }));
            } else {
                lowered.push(StructField {
                    name: generic_embedded_field_name(syntax).ok_or_else(|| {
                        Diagnostic::semantic("invalid embedded struct field type", self.source)
                    })?,
                    ty,
                    embedded: true,
                    tag,
                });
            }
        }
        Ok(Ty::Struct(lowered))
    }

    fn lower_interface(
        &mut self,
        fields: &FieldListSyntax,
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<Ty, Diagnostic> {
        let mut lowered = BTreeMap::<String, Signature>::new();
        for field in &*fields.fields {
            let syntax = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("interface element has no type"))?;
            if let Some(names) = &field.names {
                let Ty::Function(signature) = self.lower(syntax, aliases, guard_depth)? else {
                    return Err(Diagnostic::semantic(
                        "interface methods require function signatures",
                        self.source,
                    ));
                };
                for name in &**names {
                    if lowered
                        .insert(name.name.to_string(), signature.clone())
                        .is_some()
                    {
                        return Err(Diagnostic::semantic(
                            format!("duplicate interface method {}", name.name),
                            self.source,
                        ));
                    }
                }
            } else {
                let embedded = self.lower(syntax, aliases, guard_depth)?;
                let Ty::Interface(methods) = embedded.underlying() else {
                    return Err(Diagnostic::unsupported(
                        "interface type-set elements are not yet implemented",
                        self.source,
                    ));
                };
                for method in methods {
                    lowered
                        .entry(method.name.clone())
                        .or_insert_with(|| method.signature.clone());
                }
            }
        }
        Ok(Ty::Interface(
            lowered
                .into_iter()
                .map(|(name, signature)| InterfaceMethod { name, signature })
                .collect(),
        ))
    }

    fn instantiate(
        &mut self,
        base: &ExprSyntax,
        arguments: &[ExprSyntax],
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<Ty, Diagnostic> {
        let ExprSyntaxKind::Ident(base) = &base.kind else {
            return Err(Diagnostic::unsupported(
                "parameterized type base must be a named type",
                self.source,
            ));
        };
        let generic = self
            .generic_types
            .get(base.name.as_ref())
            .cloned()
            .ok_or_else(|| {
                Diagnostic::semantic(format!("{} is not a generic type", base.name), self.source)
            })?;
        recursion::ensure_finite_instantiation(generic.id, self.generic_types, self.source)?;
        let parameters = type_parameter_names_in_order(&generic.type_parameters, self.source)?;
        if generic.alias
            && direct_alias_parameter(&generic.underlying)
                .is_some_and(|name| parameters.iter().any(|parameter| parameter == name))
        {
            return Err(Diagnostic::semantic(
                "a generic alias cannot use one of its own type parameters as its direct target",
                self.source,
            ));
        }
        if parameters.len() != arguments.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "generic type {} requires {} type arguments; got {}",
                    base.name,
                    parameters.len(),
                    arguments.len()
                ),
                self.source,
            ));
        }
        let mut substitutions = BTreeMap::new();
        let mut ordered_arguments = Vec::with_capacity(arguments.len());
        for (parameter, argument) in parameters.iter().zip(arguments) {
            let argument = self.lower(argument, aliases, guard_depth)?;
            // An anonymous aggregate argument only needs an encodable identity
            // once a value is actually stored behind an interface. That check is
            // already lazy in `Ty::dynamic_type_identity`, and every consumer
            // turns its `None` into a diagnostic at the real use site, so
            // rejecting here would refuse legal types like `Box[struct{ Used }]`.
            if parameter != "_" {
                substitutions.insert(parameter.clone(), argument.clone());
            }
            ordered_arguments.push(argument);
        }
        validate_constraints(
            &generic.type_parameters,
            &substitutions,
            Some(&ordered_arguments),
            aliases,
            self.generic_types,
            self.methods,
            self.source,
        )?;
        let identity = NamedTypeId::new(generic.id, ordered_arguments);
        let same_definition = self
            .active
            .iter()
            .rev()
            .filter(|active| active.identity.definition() == identity.definition())
            .collect::<Vec<_>>();
        if let Some(active) = same_definition
            .iter()
            .copied()
            .find(|active| active.identity.is_identical_to(&identity))
        {
            if active.alias || generic.alias {
                return Err(Diagnostic::semantic(
                    format!("generic type alias cycle involving {}", base.name),
                    self.source,
                ));
            }
            if guard_depth > active.guard_depth {
                return Ok(Ty::NamedRef { identity });
            }
            return Err(Diagnostic::semantic(
                format!("invalid recursive named type {}", base.name),
                self.source,
            ));
        }
        if let Some(active) = same_definition.first().copied() {
            if active.alias || generic.alias {
                return Err(Diagnostic::semantic(
                    format!("generic type alias cycle involving {}", base.name),
                    self.source,
                ));
            }
        }
        let mut instantiated_aliases = aliases.clone();
        instantiated_aliases.extend(substitutions);
        self.active.push(ActiveInstantiation {
            identity: identity.clone(),
            guard_depth,
            alias: generic.alias,
        });
        let lowered = self.lower(&generic.underlying, &instantiated_aliases, guard_depth);
        self.active.pop();
        let underlying = lowered?;
        if generic.alias {
            Ok(underlying)
        } else {
            Ok(Ty::Named {
                identity,
                underlying: Box::new(underlying.underlying().clone()),
            })
        }
    }

    fn field_types(
        &mut self,
        fields: &FieldListSyntax,
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<Vec<Ty>, Diagnostic> {
        let mut result = Vec::new();
        for field in &*fields.fields {
            if field.variadic {
                return Err(Diagnostic::semantic(
                    "result parameters cannot be variadic",
                    self.source,
                ));
            }
            let expression = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
            let ty = self.lower(expression, aliases, guard_depth)?;
            result.extend(std::iter::repeat_n(
                ty,
                field.names.as_ref().map_or(1, |names| names.len()),
            ));
        }
        Ok(result)
    }

    fn parameter_types(
        &mut self,
        fields: &FieldListSyntax,
        aliases: &BTreeMap<String, Ty>,
        guard_depth: usize,
    ) -> Result<(Vec<Ty>, bool), Diagnostic> {
        let mut result = Vec::new();
        let mut variadic = false;
        for (index, field) in fields.fields.iter().enumerate() {
            let expression = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
            let mut ty = self.lower(expression, aliases, guard_depth)?;
            let count = field.names.as_ref().map_or(1, |names| names.len());
            if field.variadic {
                if variadic || index + 1 != fields.fields.len() || count != 1 {
                    return Err(Diagnostic::semantic(
                        "a variadic parameter must be the final single parameter",
                        self.source,
                    ));
                }
                variadic = true;
                ty = Ty::Slice(Box::new(ty));
            }
            result.extend(std::iter::repeat_n(ty, count));
        }
        Ok((result, variadic))
    }
}

fn generic_embedded_field_name(expression: &ExprSyntax) -> Option<String> {
    match &expression.kind {
        ExprSyntaxKind::Ident(ident) => Some(ident.name.to_string()),
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            token: Token::MUL,
            expression: inner,
        }
        | ExprSyntaxKind::Index { base: inner, .. }
        | ExprSyntaxKind::IndexList { base: inner, .. } => generic_embedded_field_name(inner),
        ExprSyntaxKind::Selector { member, .. } => Some(member.name.to_string()),
        _ => None,
    }
}

fn direct_alias_parameter(expression: &ExprSyntax) -> Option<&str> {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => direct_alias_parameter(inner),
        ExprSyntaxKind::Ident(ident) => Some(ident.name.as_ref()),
        _ => None,
    }
}

pub(in crate::compiler::semantic) fn type_contains_instantiation(expression: &ExprSyntax) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Index { .. } | ExprSyntaxKind::IndexList { .. } => true,
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            expression: inner, ..
        } => type_contains_instantiation(inner),
        ExprSyntaxKind::ArrayType { element, .. } | ExprSyntaxKind::ChannelType { element, .. } => {
            type_contains_instantiation(element)
        }
        ExprSyntaxKind::MapType { key, value } => {
            type_contains_instantiation(key) || type_contains_instantiation(value)
        }
        ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            field_list_contains_instantiation(params)
                || results
                    .as_ref()
                    .is_some_and(field_list_contains_instantiation)
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            field_list_contains_instantiation(fields)
        }
        ExprSyntaxKind::Ident(_)
        | ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::TypeAssert { .. }
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Binary { .. }
        | ExprSyntaxKind::Call { .. }
        | ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => false,
    }
}

fn field_list_contains_instantiation(fields: &FieldListSyntax) -> bool {
    fields
        .fields
        .iter()
        .any(|field| field.ty.as_ref().is_some_and(type_contains_instantiation))
}

pub(super) fn instantiate_generic_receiver(
    selected_ty: &Ty,
    pointer: bool,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    let receiver = match selected_ty {
        Ty::Named { .. } => selected_ty.clone(),
        Ty::Pointer(element) if matches!(element.as_ref(), Ty::Named { .. }) => {
            element.as_ref().clone()
        }
        _ => {
            return Err(Diagnostic::backend(format!(
                "generic method selected a non-named receiver at {source:?}"
            )));
        }
    };
    Ok(if pointer {
        Ty::Pointer(Box::new(receiver))
    } else {
        receiver
    })
}

pub(super) fn instantiate_signature(
    header: &FunctionHeaderSyntax,
    substitutions: &BTreeMap<String, Ty>,
    receiver_override: Option<Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<Signature, Diagnostic> {
    let mut aliases = aliases.clone();
    aliases.extend(substitutions.clone());
    let mut params = Vec::new();
    if let Some(receiver) = &header.receiver {
        if let Some(receiver) = receiver_override {
            params.push(receiver);
        } else {
            let (receiver, variadic) =
                parameter_types_with_generics(receiver, &aliases, generic_types, methods, source)?;
            if variadic || receiver.len() != 1 {
                return Err(Diagnostic::semantic(
                    "a method must declare exactly one non-variadic receiver",
                    source,
                ));
            }
            params.extend(receiver);
        }
    } else if receiver_override.is_some() {
        return Err(Diagnostic::backend(
            "generic function received a method receiver override",
        ));
    }
    let (ordinary, variadic) =
        parameter_types_with_generics(&header.params, &aliases, generic_types, methods, source)?;
    params.extend(ordinary);
    let results = header
        .results
        .as_ref()
        .map(|results| field_types_with_generics(results, &aliases, generic_types, methods, source))
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
    methods: MethodEnvironment<'_>,
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
        let ty = lower_type_with_generics(expression, aliases, generic_types, methods, source)?;
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
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = false;
    for (index, field) in fields.fields.iter().enumerate() {
        let expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let mut ty = lower_type_with_generics(expression, aliases, generic_types, methods, source)?;
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

pub(super) fn named_receiver_identity(ty: &Ty) -> Option<&NamedTypeId> {
    match ty {
        Ty::Named { identity, .. } => Some(identity),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { identity, .. } => Some(identity),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn named_receiver_definition(ty: &Ty) -> Option<crate::compiler::ids::QualifiedDefId> {
    named_receiver_identity(ty).map(NamedTypeId::definition)
}

pub(super) fn instantiated_receiver_substitutions(
    generic: &GenericTypeSymbol,
    ty: &Ty,
    source: SourceRef,
) -> Result<BTreeMap<String, Ty>, Diagnostic> {
    let identity = named_receiver_identity(ty).ok_or_else(|| {
        Diagnostic::backend("generic method selected a non-named receiver representation")
    })?;
    if identity.definition() != generic.id {
        return Err(Diagnostic::backend(
            "generic method selected a receiver from another definition",
        ));
    }
    let parameters = type_parameter_names_in_order(&generic.type_parameters, source)?;
    if parameters.len() != identity.arguments().len() {
        return Err(Diagnostic::backend(
            "generic receiver identity has the wrong type-argument arity",
        ));
    }
    for argument in identity.arguments() {
        ensure_stable_identity_argument(argument, source)?;
    }
    Ok(parameters
        .into_iter()
        .zip(identity.arguments().iter().cloned())
        .filter(|(parameter, _)| parameter != "_")
        .collect())
}

fn ensure_stable_identity_argument(ty: &Ty, source: SourceRef) -> Result<(), Diagnostic> {
    let unsupported = match ty {
        Ty::LocalNamed { .. } => Some("function-local named type"),
        Ty::Struct(_) => Some("anonymous struct type"),
        Ty::Interface(_) => Some("anonymous interface type"),
        Ty::Unit | Ty::Tuple(_) | Ty::Untyped(_) => Some("non-source type"),
        Ty::Named { identity, .. } | Ty::NamedRef { identity } => {
            for argument in identity.arguments() {
                ensure_stable_identity_argument(argument, source)?;
            }
            None
        }
        Ty::Pointer(element)
        | Ty::Array(_, element)
        | Ty::Slice(element)
        | Ty::Channel(_, element) => {
            ensure_stable_identity_argument(element, source)?;
            None
        }
        Ty::Map(key, value) => {
            ensure_stable_identity_argument(key, source)?;
            ensure_stable_identity_argument(value, source)?;
            None
        }
        Ty::Function(signature) => {
            for ty in signature.params.iter().chain(&signature.results) {
                ensure_stable_identity_argument(ty, source)?;
            }
            None
        }
        Ty::Bool | Ty::Int(_) | Ty::Uint(_) | Ty::Float(_) | Ty::Complex(_) | Ty::String => None,
    };
    if let Some(kind) = unsupported {
        return Err(Diagnostic::unsupported(
            format!(
                "generic type argument uses {kind}, whose exact package-owned identity is not yet modeled"
            ),
            source,
        ));
    }
    Ok(())
}
