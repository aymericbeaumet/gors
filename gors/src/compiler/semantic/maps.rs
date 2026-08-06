//! Typed lowering for the `map[string]int` runtime representation.

use super::FunctionLowerer;
use super::channels::{channel_effects, int_channel_parts};
use super::expressions::{coerce_expr, expr_constant};
use super::interfaces::dynamic_type_identity;
use super::lower_type;
use super::pointers::{int_pointer_ty, pointer_effects};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, IntTy, Ty, UintTy, UntypedTy};

pub(super) fn string_i64_map_ty() -> Ty {
    Ty::Map(Box::new(Ty::String), Box::new(Ty::Int(IntTy::Int)))
}

fn string_aggregate_map_value_ty(ty: &Ty) -> Option<&Ty> {
    let Ty::Map(key, value) = ty.underlying() else {
        return None;
    };
    (key.underlying() == &Ty::String && value.bootstrap_i64_struct_fields().is_some())
        .then_some(value)
}

impl FunctionLowerer {
    pub(super) fn try_lower_map_comma_ok(
        &mut self,
        expression: &ExprSyntax,
    ) -> Option<Result<hir::Expr, Diagnostic>> {
        let ExprSyntaxKind::Index { base, index } = &expression.kind else {
            return None;
        };
        Some((|| {
            let node = self.alloc_node(expression.source)?;
            let source = SourceRef::node(node);
            let map = self.lower_expr(base, None)?;
            if map.ty.underlying() != string_i64_map_ty().underlying() {
                return Err(Diagnostic::semantic(
                    "comma-ok assignment requires a map lookup",
                    source,
                ));
            }
            let key = self.lower_expr(index, Some(&Ty::String))?;
            let effects = map_effects(&[&map, &key], false, false, false);
            Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::MapStringI64Lookup),
                    args: vec![map, key],
                },
                ty: Ty::Tuple(vec![Ty::Int(IntTy::Int), Ty::Bool]),
                category: hir::ValueCategory::Value,
                effects,
                source,
            })
        })())
    }

    pub(super) fn zero_value_expr(
        &self,
        node: NodeId,
        source: SourceRef,
        ty: Ty,
    ) -> Result<hir::Expr, Diagnostic> {
        if matches!(ty.underlying(), Ty::Interface(_)) {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::InterfaceNil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: map_effects(&[], false, false, false),
                source,
            });
        }
        if ty.underlying() == int_pointer_ty().underlying() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::PointerI64Nil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: pointer_effects(&[], false, false, false),
                source,
            });
        }
        if ty.bootstrap_i64_struct_pointer_fields().is_some() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Nil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: pointer_effects(&[], false, false, false),
                source,
            });
        }
        if ty.underlying() == string_i64_map_ty().underlying() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::MapStringI64Nil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: map_effects(&[], false, false, false),
                source,
            });
        }
        if int_channel_parts(&ty).is_some() {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::ChannelI64Nil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: channel_effects(&[], false, false, false, false),
                source,
            });
        }
        if let Ty::Struct(fields) = ty.underlying() {
            let values = fields
                .iter()
                .map(|field| self.zero_value_expr(node, source, field.ty.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::StructLiteral(values),
                ty,
                category: hir::ValueCategory::Value,
                effects: hir::Effects::default(),
                source,
            });
        }
        let value = ty.zero().ok_or_else(|| {
            Diagnostic::unsupported(format!("zero value for {ty:?} is not implemented"), source)
        })?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Constant(value),
            ty,
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source,
        })
    }

    pub(super) fn lower_map_literal(
        &mut self,
        literal_ty: Ty,
        elements: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let aggregate_value = string_aggregate_map_value_ty(&literal_ty).cloned();
        if literal_ty.underlying() != string_i64_map_ty().underlying() && aggregate_value.is_none()
        {
            return Err(Diagnostic::unsupported(
                "map literal key/value types have no executable representation",
                source,
            ));
        }
        let mut entries = Vec::with_capacity(elements.len());
        let mut effects = map_effects(&[], true, true, false);
        for element in elements {
            let ExprSyntaxKind::KeyValue { key, value } = &element.kind else {
                return Err(Diagnostic::semantic(
                    "map literal elements require key: value syntax",
                    source,
                ));
            };
            let key = self.lower_expr(key, Some(&Ty::String))?;
            let value = self.lower_expr(
                value,
                Some(aggregate_value.as_ref().unwrap_or(&Ty::Int(IntTy::Int))),
            )?;
            effects = effects.union(key.effects).union(value.effects);
            entries.push((key, value));
        }
        let kind = if let Some(value_ty) = aggregate_value {
            let type_identity = dynamic_type_identity(&value_ty).ok_or_else(|| {
                Diagnostic::backend("aggregate map value omitted its dynamic type identity")
            })?;
            hir::ExprKind::AggregateMapLiteral {
                entries,
                type_identity,
            }
        } else {
            hir::ExprKind::MapLiteralStringI64(entries)
        };
        Ok(hir::Expr {
            node,
            kind,
            ty: literal_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn lower_map_index(
        &mut self,
        map: hir::Expr,
        index: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let result_ty = match map.ty.underlying() {
            Ty::Map(key, value) if key.underlying() == &Ty::String => value.as_ref().clone(),
            _ => {
                return Err(Diagnostic::backend(
                    "map index lowering received a non-string-keyed map",
                ));
            }
        };
        let aggregate_identity = (result_ty.bootstrap_i64_struct_fields().is_some())
            .then(|| dynamic_type_identity(&result_ty))
            .flatten();
        if result_ty.underlying() != &Ty::Int(IntTy::Int) && aggregate_identity.is_none() {
            return Err(Diagnostic::unsupported(
                "map value type has no executable lookup representation",
                source,
            ));
        }
        let key = self.lower_expr(index, Some(&Ty::String))?;
        let effects = map_effects(&[&map, &key], false, false, false);
        let mut result = hir::Expr {
            node,
            kind: if let Some(type_identity) = aggregate_identity {
                hir::ExprKind::AggregateMapIndex {
                    map: Box::new(map),
                    key: Box::new(key),
                    type_identity,
                }
            } else {
                hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::MapStringI64Get),
                    args: vec![map, key],
                }
            },
            ty: result_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_make_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Some(declared_syntax) = arguments.first() else {
            return Err(Diagnostic::semantic(
                "make requires a type argument",
                source,
            ));
        };
        let declared = lower_type(declared_syntax, &self.type_aliases, source)?;
        if int_channel_parts(&declared).is_some() {
            return self.lower_channel_make(declared, arguments, spread, node, source, expected);
        }
        let aggregate_map = string_aggregate_map_value_ty(&declared).is_some();
        if declared.underlying() != string_i64_map_ty().underlying() && !aggregate_map {
            return self
                .lower_slice_builtin_call("make", arguments, spread, node, source, expected);
        }
        if spread || arguments.len() != 1 {
            return Err(Diagnostic::unsupported(
                "make of a map currently requires no capacity hint",
                source,
            ));
        }
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(if aggregate_map {
                    hir::Builtin::AggregateMapMake
                } else {
                    hir::Builtin::MapStringI64Make
                }),
                args: Vec::new(),
            },
            ty: declared,
            category: hir::ValueCategory::Value,
            effects: map_effects(&[], false, true, false),
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_len_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [value] = arguments else {
            return Err(Diagnostic::semantic(
                "len requires exactly one argument",
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic("len does not accept ...", source));
        }
        let value = self.lower_expr(value, None)?;
        if let Some(ConstValue::String(bytes)) = expr_constant(&value) {
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::Constant(ConstValue::Int(bytes.len().to_string())),
                ty: Ty::Untyped(UntypedTy::Int),
                category: hir::ValueCategory::Constant,
                effects: hir::Effects::default(),
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        if int_channel_parts(&value.ty).is_some() {
            return self.lower_channel_len(value, node, source, expected);
        }
        if let Ty::Array(length, element) = value.ty.underlying()
            && super::arrays::is_executable_array_element(element)
        {
            let length = *length;
            return self.lower_array_len(value, length, node, source, expected);
        }
        let builtin = match value.ty.underlying() {
            Ty::Map(key, element)
                if key.underlying() == &Ty::String
                    && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                hir::Builtin::MapStringI64Len
            }
            Ty::Map(key, element)
                if key.underlying() == &Ty::String
                    && element.bootstrap_i64_struct_fields().is_some() =>
            {
                hir::Builtin::AggregateMapLen
            }
            Ty::Slice(element)
                if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
            {
                hir::Builtin::SliceI64Len
            }
            Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
                hir::Builtin::SliceU8Len
            }
            Ty::Slice(element) if element.bootstrap_i64_struct_fields().is_some() => {
                hir::Builtin::AggregateSliceLen
            }
            Ty::String => hir::Builtin::StringLen,
            ty => {
                return Err(Diagnostic::unsupported(
                    format!("len is not yet implemented for {ty:?}"),
                    source,
                ));
            }
        };
        let effects = map_effects(&[&value], false, false, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args: vec![value],
            },
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_clear_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [value] = arguments else {
            return Err(Diagnostic::semantic(
                "clear requires exactly one argument",
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic("clear does not accept ...", source));
        }
        let value = self.lower_expr(value, None)?;
        let builtin = match value.ty.underlying() {
            Ty::Map(key, element)
                if key.underlying() == &Ty::String
                    && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                hir::Builtin::MapStringI64Clear
            }
            Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                hir::Builtin::SliceI64Clear
            }
            ty => {
                return Err(Diagnostic::unsupported(
                    format!("clear is not yet implemented for {ty:?}"),
                    source,
                ));
            }
        };
        let effects = map_effects(&[&value], true, false, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args: vec![value],
            },
            ty: Ty::Unit,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_delete_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [map, key] = arguments else {
            return Err(Diagnostic::semantic(
                "delete requires a map and key",
                source,
            ));
        };
        if spread {
            return Err(Diagnostic::semantic("delete does not accept ...", source));
        }
        let map = self.lower_expr(map, Some(&string_i64_map_ty()))?;
        let key = self.lower_expr(key, Some(&Ty::String))?;
        let effects = map_effects(&[&map, &key], true, false, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::MapStringI64Delete),
                args: vec![map, key],
            },
            ty: Ty::Unit,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

fn map_effects(
    arguments: &[&hir::Expr],
    writes: bool,
    allocates: bool,
    panics: bool,
) -> hir::Effects {
    arguments.iter().fold(
        hir::Effects {
            may_read: true,
            may_call: true,
            may_allocate: allocates,
            may_panic: panics,
            may_write: writes,
            ..hir::Effects::default()
        },
        |effects, argument| effects.union(argument.effects),
    )
}
