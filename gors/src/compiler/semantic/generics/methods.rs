//! Instantiation of methods declared on parameterized receiver types.

use std::collections::BTreeMap;

use super::{
    MethodEnvironment, infer_receiver_parameter_aliases, instantiate_generic_receiver,
    instantiate_signature, instantiated_receiver_substitutions, named_receiver_identity,
    single_receiver_type, validate_constraints,
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
    let Some(identity) = named_receiver_identity(selected_ty) else {
        return Ok(None);
    };
    let definition = identity.definition();
    let Some(symbol) = methods.generic.get(&(definition, name.to_owned())) else {
        return Ok(None);
    };
    let generic = generic_types
        .values()
        .find(|generic| generic.id == definition)
        .ok_or_else(|| Diagnostic::backend("generic method receiver type disappeared"))?;
    let receiver_syntax = single_receiver_type(&symbol.header, source)?;
    let receiver_aliases = infer_receiver_parameter_aliases(
        receiver_syntax,
        &generic.type_parameters,
        identity.arguments(),
        source,
    )?;
    let mut body_substitutions = instantiated_receiver_substitutions(generic, selected_ty, source)?;
    body_substitutions.extend(receiver_aliases);
    validate_constraints(
        &generic.type_parameters,
        &body_substitutions,
        Some(identity.arguments()),
        aliases,
        generic_types,
        methods,
        source,
    )?;
    let receiver_ty = instantiate_generic_receiver(selected_ty, symbol.pointer_receiver, source)?;
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
