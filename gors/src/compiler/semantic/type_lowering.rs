//! Go type lowering over resolved aliases and exact constants.

use std::collections::BTreeMap;

use super::{ConstantSymbol, arrays, interfaces};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ChannelDirectionSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax,
};
use crate::compiler::types::{
    ChannelDir, ConstValue, FloatTy, IntTy, InterfaceMethod, Signature, StructField, Ty, UintTy,
};

pub(super) fn predeclared_constant_type(name: &str) -> Option<Ty> {
    Some(match name {
        "bool" => Ty::Bool,
        "string" => Ty::String,
        "int" => Ty::Int(IntTy::Int),
        "int8" => Ty::Int(IntTy::Int8),
        "int32" | "rune" => Ty::Int(IntTy::Int32),
        "uint" => Ty::Uint(UintTy::Uint),
        "float32" => Ty::Float(FloatTy::Float32),
        "float64" => Ty::Float(FloatTy::Float64),
        "complex128" => Ty::Complex(crate::compiler::types::ComplexTy::Complex128),
        "uint8" | "byte" => Ty::Uint(UintTy::Uint8),
        _ => return None,
    })
}

pub(super) fn field_types(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<Vec<Ty>, Diagnostic> {
    field_types_with_constant_lookup(fields, type_aliases, &|_| None, source)
}

pub(super) fn field_types_with_constant_lookup(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
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
        let type_expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let ty = lower_type_with_constant_lookup(
            type_expression,
            type_aliases,
            constant_lookup,
            source,
        )?;
        let count = field.names.as_ref().map_or(1, |names| names.len());
        result.extend(std::iter::repeat_n(ty, count));
    }
    Ok(result)
}

pub(super) fn parameter_types(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    parameter_types_with_constant_lookup(fields, type_aliases, &|_| None, source)
}

pub(super) fn parameter_types_with_constant_lookup(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = false;
    for (index, field) in fields.fields.iter().enumerate() {
        let type_expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let mut ty = lower_type_with_constant_lookup(
            type_expression,
            type_aliases,
            constant_lookup,
            source,
        )?;
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

pub(in crate::compiler) fn lower_type(
    expression: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    lower_type_with_constant_lookup(expression, type_aliases, &|_| None, source)
}

pub(in crate::compiler) fn lower_type_with_constants(
    expression: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constants: &BTreeMap<String, ConstantSymbol>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    lower_type_with_constant_lookup(
        expression,
        type_aliases,
        &|name| {
            constants
                .get(name)
                .map(|constant| (constant.ty.clone(), constant.value.clone()))
        },
        source,
    )
}

pub(in crate::compiler) fn lower_type_with_constant_lookup(
    expression: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    if let ExprSyntaxKind::Paren(expression) = &expression.kind {
        return lower_type_with_constant_lookup(expression, type_aliases, constant_lookup, source);
    }
    if let ExprSyntaxKind::ArrayType { length, element } = &expression.kind {
        return match length {
            None => Ok(Ty::Slice(Box::new(lower_type_with_constant_lookup(
                element,
                type_aliases,
                constant_lookup,
                source,
            )?))),
            Some(length) => {
                arrays::lower_array_type(length, element, type_aliases, constant_lookup, source)
            }
        };
    }
    if let ExprSyntaxKind::MapType { key, value } = &expression.kind {
        return Ok(Ty::Map(
            Box::new(lower_type_with_constant_lookup(
                key,
                type_aliases,
                constant_lookup,
                source,
            )?),
            Box::new(lower_type_with_constant_lookup(
                value,
                type_aliases,
                constant_lookup,
                source,
            )?),
        ));
    }
    if let ExprSyntaxKind::ChannelType { direction, element } = &expression.kind {
        let direction = match direction {
            ChannelDirectionSyntax::SendReceive => ChannelDir::SendReceive,
            ChannelDirectionSyntax::SendOnly => ChannelDir::SendOnly,
            ChannelDirectionSyntax::ReceiveOnly => ChannelDir::ReceiveOnly,
        };
        return Ok(Ty::Channel(
            direction,
            Box::new(lower_type_with_constant_lookup(
                element,
                type_aliases,
                constant_lookup,
                source,
            )?),
        ));
    }
    if let ExprSyntaxKind::Unary {
        token: crate::token::Token::MUL,
        expression,
    } = &expression.kind
    {
        return Ok(Ty::Pointer(Box::new(lower_type_with_constant_lookup(
            expression,
            type_aliases,
            constant_lookup,
            source,
        )?)));
    }
    if let ExprSyntaxKind::FunctionType {
        has_type_parameters,
        params,
        results,
    } = &expression.kind
    {
        if *has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic function types are not yet implemented",
                source,
            ));
        }
        let (params, variadic) =
            parameter_types_with_constant_lookup(params, type_aliases, constant_lookup, source)?;
        let results = results
            .as_ref()
            .map(|results| {
                field_types_with_constant_lookup(results, type_aliases, constant_lookup, source)
            })
            .transpose()?
            .unwrap_or_default();
        return Ok(Ty::Function(Signature {
            params,
            results,
            variadic,
        }));
    }
    if let ExprSyntaxKind::StructType { fields } = &expression.kind {
        let mut lowered = Vec::new();
        for field in &*fields.fields {
            if field.variadic {
                return Err(Diagnostic::semantic(
                    "struct fields cannot be variadic",
                    source,
                ));
            }
            let syntax = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("struct field has no type"))?;
            let ty =
                lower_type_with_constant_lookup(syntax, type_aliases, constant_lookup, source)?;
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
                    name: embedded_field_name(syntax).ok_or_else(|| {
                        Diagnostic::semantic("invalid embedded struct field type", source)
                    })?,
                    ty,
                    embedded: true,
                    tag,
                });
            }
        }
        return Ok(Ty::Struct(lowered));
    }
    if let ExprSyntaxKind::InterfaceType { methods } = &expression.kind {
        let mut lowered = BTreeMap::<String, Signature>::new();
        for field in &*methods.fields {
            let syntax = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("interface element has no type"))?;
            if let Some(names) = &field.names {
                let Ty::Function(signature) =
                    lower_type_with_constant_lookup(syntax, type_aliases, constant_lookup, source)?
                else {
                    return Err(Diagnostic::semantic(
                        "interface methods require function signatures",
                        source,
                    ));
                };
                for name in &**names {
                    if lowered
                        .insert(name.name.to_string(), signature.clone())
                        .is_some()
                    {
                        return Err(Diagnostic::semantic(
                            format!("duplicate interface method {}", name.name),
                            source,
                        ));
                    }
                }
            } else {
                let embedded =
                    lower_type_with_constant_lookup(syntax, type_aliases, constant_lookup, source)?;
                let Ty::Interface(methods) = embedded.underlying() else {
                    return Err(Diagnostic::unsupported(
                        "interface type-set elements are not yet implemented",
                        source,
                    ));
                };
                for method in methods {
                    lowered
                        .entry(method.name.clone())
                        .or_insert_with(|| method.signature.clone());
                }
            }
        }
        return Ok(Ty::Interface(
            lowered
                .into_iter()
                .map(|(name, signature)| InterfaceMethod { name, signature })
                .collect(),
        ));
    }
    let ExprSyntaxKind::Ident(ident) = &expression.kind else {
        return Err(Diagnostic::unsupported(
            "this Go type is not yet supported",
            source,
        ));
    };
    if let Some(ty) = predeclared_constant_type(ident.name.as_ref()) {
        return Ok(ty);
    }
    match ident.name.as_ref() {
        "any" => Ok(Ty::Interface(Vec::new())),
        "error" => Ok(interfaces::error_interface_ty()),
        "int16" | "int64" | "uint16" | "uint32" | "uint64" | "uintptr" | "complex64" => {
            Err(Diagnostic::unsupported(
                format!(
                    "executable support for type {} is not yet available",
                    ident.name
                ),
                source,
            ))
        }
        other => type_aliases.get(other).cloned().ok_or_else(|| {
            Diagnostic::unsupported(format!("type {other} is not yet supported"), source)
        }),
    }
}

fn embedded_field_name(expression: &ExprSyntax) -> Option<String> {
    match &expression.kind {
        ExprSyntaxKind::Ident(ident) => Some(ident.name.to_string()),
        ExprSyntaxKind::Unary {
            token: crate::token::Token::MUL,
            expression,
        } => embedded_field_name(expression),
        ExprSyntaxKind::Selector { member, .. } => Some(member.name.to_string()),
        _ => None,
    }
}
