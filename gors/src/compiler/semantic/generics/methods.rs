//! Instantiation of methods declared on parameterized receiver types.

use std::collections::BTreeMap;

use super::{
    MethodEnvironment, infer_receiver_parameter_aliases, infer_type_expression,
    instantiate_generic_receiver, instantiate_signature, named_receiver_definition,
    single_receiver_type, type_parameter_names, type_parameter_names_in_order,
    validate_constraints,
};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::{GenericTypeSymbol, MethodSymbol};
use crate::compiler::types::Ty;

pub(super) fn instantiate_generic_method_for_receiver(
    selected_ty: &Ty,
    name: &str,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<Option<MethodSymbol>, Diagnostic> {
    let Some(definition) = named_receiver_definition(selected_ty) else {
        return Ok(None);
    };
    let Some(symbol) = methods.generic.get(&(definition, name.to_owned())) else {
        return Ok(None);
    };
    let generic = generic_types
        .values()
        .find(|generic| generic.id == definition)
        .ok_or_else(|| Diagnostic::backend("generic method receiver type disappeared"))?;
    let receiver_syntax = single_receiver_type(&symbol.header, source)?;
    let inference_ty = match (symbol.pointer_receiver, selected_ty) {
        (true, Ty::Pointer(_)) | (false, Ty::Named { .. }) => selected_ty.clone(),
        (true, selected) => Ty::Pointer(Box::new(selected.clone())),
        (false, Ty::Pointer(selected)) => selected.as_ref().clone(),
        (false, selected) => selected.clone(),
    };
    let receiver_parameters = type_parameter_names(&generic.type_parameters, source)?;
    let mut substitutions = BTreeMap::new();
    infer_type_expression(
        receiver_syntax,
        &inference_ty,
        &receiver_parameters,
        &mut substitutions,
        aliases,
        generic_types,
        methods,
        source,
    )?;
    let missing = type_parameter_names_in_order(&generic.type_parameters, source)?
        .into_iter()
        .filter(|parameter| parameter != "_" && !substitutions.contains_key(parameter))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::unsupported(
            format!(
                "cannot recover generic receiver type arguments {} for method {name} from the instantiated receiver representation",
                missing.join(", ")
            ),
            source,
        ));
    }
    let receiver_aliases = infer_receiver_parameter_aliases(
        receiver_syntax,
        &generic.type_parameters,
        &substitutions,
        source,
    )?;
    let mut body_substitutions = substitutions.clone();
    body_substitutions.extend(receiver_aliases);
    validate_constraints(
        &generic.type_parameters,
        &body_substitutions,
        aliases,
        generic_types,
        methods,
        source,
    )?;
    let receiver_ty = instantiate_generic_receiver(
        generic,
        &substitutions,
        symbol.pointer_receiver,
        aliases,
        generic_types,
        methods,
        source,
    )?;
    let signature = instantiate_signature(
        &symbol.header,
        &body_substitutions,
        Some(receiver_ty),
        aliases,
        generic_types,
        methods,
        source,
    )?;
    Ok(Some(MethodSymbol {
        id: symbol.id,
        signature,
        pointer_receiver: symbol.pointer_receiver,
    }))
}
