//! Typed lowering for concrete map runtime representations.

use super::FunctionLowerer;
use super::channels::{channel_effects, channel_parts};
use super::expressions::coerce_expr;
use super::pointers::pointer_effects;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty, UintTy};

pub(super) fn string_i64_map_ty() -> Ty {
    Ty::Map(Box::new(Ty::String), Box::new(Ty::Int(IntTy::Int)))
}

pub(super) fn i64_go_string_map_ty() -> Ty {
    Ty::Map(Box::new(Ty::Int(IntTy::Int)), Box::new(Ty::String))
}

#[derive(Clone, Copy)]
enum ConcreteMapRepresentation {
    StringI64,
    I64GoString,
}

impl ConcreteMapRepresentation {
    fn for_ty(ty: &Ty) -> Option<Self> {
        if ty.underlying() == string_i64_map_ty().underlying() {
            Some(Self::StringI64)
        } else if ty.underlying() == i64_go_string_map_ty().underlying() {
            Some(Self::I64GoString)
        } else {
            None
        }
    }

    fn key_ty(self) -> Ty {
        match self {
            Self::StringI64 => Ty::String,
            Self::I64GoString => Ty::Int(IntTy::Int),
        }
    }

    fn value_ty(self) -> Ty {
        match self {
            Self::StringI64 => Ty::Int(IntTy::Int),
            Self::I64GoString => Ty::String,
        }
    }

    fn nil(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Nil,
            Self::I64GoString => hir::Builtin::MapI64GoStringNil,
        }
    }

    fn make(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Make,
            Self::I64GoString => hir::Builtin::MapI64GoStringMake,
        }
    }

    fn get(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Get,
            Self::I64GoString => hir::Builtin::MapI64GoStringGet,
        }
    }

    fn lookup(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Lookup,
            Self::I64GoString => hir::Builtin::MapI64GoStringLookup,
        }
    }

    fn delete(self) -> hir::Builtin {
        match self {
            Self::StringI64 => hir::Builtin::MapStringI64Delete,
            Self::I64GoString => hir::Builtin::MapI64GoStringDelete,
        }
    }
}

fn string_aggregate_map_value_ty(ty: &Ty) -> Option<&Ty> {
    let Ty::Map(key, value) = ty.underlying() else {
        return None;
    };
    (key.underlying() == &Ty::String
        && value.bootstrap_i64_struct_fields().is_some()
        && value.dynamic_type_identity().is_some())
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
            let Some(representation) = ConcreteMapRepresentation::for_ty(&map.ty) else {
                return Err(Diagnostic::semantic(
                    "comma-ok assignment requires a map lookup",
                    source,
                ));
            };
            let key = self.lower_expr(index, Some(&representation.key_ty()))?;
            let effects = map_effects(&[&map, &key], false, false, false);
            Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(representation.lookup()),
                    args: vec![map, key],
                },
                ty: Ty::Tuple(vec![representation.value_ty(), Ty::Bool]),
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
        let slice_nil = match ty.underlying() {
            Ty::Slice(element)
                if element.uses_i64_slice_carrier()
                    || element.snapshot_function_result().is_some() =>
            {
                Some(hir::Builtin::SliceI64Nil)
            }
            Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
                Some(hir::Builtin::SliceU8Nil)
            }
            Ty::Slice(element) if element.underlying() == &Ty::Bool => {
                Some(hir::Builtin::SliceBoolNil)
            }
            Ty::Slice(element) if element.underlying() == &Ty::String => {
                Some(hir::Builtin::SliceGoStringNil)
            }
            Ty::Slice(element)
                if matches!(element.underlying(), Ty::Interface(_))
                    || element.uses_interface_aggregate_representation() =>
            {
                Some(hir::Builtin::AggregateSliceNil)
            }
            _ => None,
        };
        if let Some(builtin) = slice_nil {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(builtin),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: map_effects(&[], false, true, false),
                source,
            });
        }
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
        if matches!(ty.underlying(), Ty::Function(_)) {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::FunctionNil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: map_effects(&[], false, false, false),
                source,
            });
        }
        if matches!(
            ty.underlying(),
            Ty::Pointer(element) if element.underlying() == &Ty::Int(IntTy::Int)
        ) {
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
        if matches!(
            ty.underlying(),
            Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation()
        ) {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::AggregatePointerNil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: pointer_effects(&[], false, false, false),
                source,
            });
        }
        if let Some(representation) = ConcreteMapRepresentation::for_ty(&ty) {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(representation.nil()),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: map_effects(&[], false, false, false),
                source,
            });
        }
        if let Some((_, _, representation)) = channel_parts(&ty) {
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(representation.builtins().nil),
                    args: Vec::new(),
                },
                ty,
                category: hir::ValueCategory::Value,
                effects: channel_effects(&[], false, false, false, false),
                source,
            });
        }
        if let Ty::Array(length, element) = ty.underlying()
            && super::arrays::is_executable_array(*length, element)
        {
            let length = usize::try_from(*length).map_err(|_| {
                Diagnostic::unsupported("array length does not fit this host", source)
            })?;
            if matches!(ty, Ty::Array(_, _)) && element.underlying() == &Ty::Int(IntTy::Int) {
                return Ok(hir::Expr {
                    node,
                    kind: hir::ExprKind::ArrayLiteralI64(vec![0; length]),
                    ty,
                    category: hir::ValueCategory::Value,
                    effects: hir::Effects::default(),
                    source,
                });
            }
            let mut values = Vec::with_capacity(length);
            for index in 0..length {
                let value = self.zero_value_expr(node, source, element.as_ref().clone())?;
                values.push((
                    u64::try_from(index)
                        .map_err(|_| Diagnostic::backend("array index does not fit u64"))?,
                    value,
                ));
            }
            return Ok(hir::Expr {
                node,
                kind: hir::ExprKind::ArrayLiteral(values),
                ty,
                category: hir::ValueCategory::Value,
                effects: hir::Effects::default(),
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
        let concrete = ConcreteMapRepresentation::for_ty(&literal_ty);
        if concrete.is_none() && aggregate_value.is_none() {
            return Err(Diagnostic::unsupported(
                "map literal key/value types have no executable representation",
                source,
            ));
        }
        let key_ty = concrete
            .map(ConcreteMapRepresentation::key_ty)
            .unwrap_or(Ty::String);
        let value_ty = concrete
            .map(ConcreteMapRepresentation::value_ty)
            .or_else(|| aggregate_value.clone())
            .ok_or_else(|| Diagnostic::backend("represented map omitted its value type"))?;
        let mut entries = Vec::with_capacity(elements.len());
        let mut effects = map_effects(&[], true, true, false);
        for element in elements {
            let ExprSyntaxKind::KeyValue { key, value } = &element.kind else {
                return Err(Diagnostic::semantic(
                    "map literal elements require key: value syntax",
                    source,
                ));
            };
            let key = self.lower_expr(key, Some(&key_ty))?;
            let value = self.lower_expr(value, Some(&value_ty))?;
            effects = effects.union(key.effects).union(value.effects);
            entries.push((key, value));
        }
        let kind = if let Some(value_ty) = aggregate_value {
            let type_identity = value_ty.dynamic_type_identity().ok_or_else(|| {
                Diagnostic::backend("aggregate map value omitted its dynamic type identity")
            })?;
            hir::ExprKind::AggregateMapLiteral {
                entries,
                type_identity,
            }
        } else {
            match concrete {
                Some(ConcreteMapRepresentation::StringI64) => {
                    hir::ExprKind::MapLiteralStringI64(entries)
                }
                Some(ConcreteMapRepresentation::I64GoString) => {
                    hir::ExprKind::MapLiteralI64GoString(entries)
                }
                None => return Err(Diagnostic::backend("represented map omitted its lowering")),
            }
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
        let (key_ty, result_ty, get) =
            if let Some(representation) = ConcreteMapRepresentation::for_ty(&map.ty) {
                (
                    representation.key_ty(),
                    representation.value_ty(),
                    Some(representation.get()),
                )
            } else {
                match map.ty.underlying() {
                    Ty::Map(key, value) if key.underlying() == &Ty::String => {
                        (Ty::String, value.as_ref().clone(), None)
                    }
                    _ => {
                        return Err(Diagnostic::backend(
                            "map index lowering received an unrepresented map",
                        ));
                    }
                }
            };
        let aggregate_identity = (result_ty.bootstrap_i64_struct_fields().is_some())
            .then(|| result_ty.dynamic_type_identity())
            .flatten();
        if get.is_none() && aggregate_identity.is_none() {
            return Err(Diagnostic::unsupported(
                "map value type has no executable lookup representation",
                source,
            ));
        }
        let key = self.lower_expr(index, Some(&key_ty))?;
        let effects = map_effects(&[&map, &key], false, false, false);
        let mut result = hir::Expr {
            node,
            kind: if let Some(type_identity) = aggregate_identity {
                hir::ExprKind::AggregateMapIndex {
                    map: Box::new(map),
                    key: Box::new(key),
                    type_identity,
                }
            } else if let Some(get) = get {
                hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(get),
                    args: vec![map, key],
                }
            } else {
                return Err(Diagnostic::backend(
                    "represented map lookup omitted its runtime operation",
                ));
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
        let declared = self.lower_scoped_type(declared_syntax, source)?;
        if channel_parts(&declared).is_some() {
            return self.lower_channel_make(declared, arguments, spread, node, source, expected);
        }
        let aggregate_map = string_aggregate_map_value_ty(&declared).is_some();
        let concrete = ConcreteMapRepresentation::for_ty(&declared);
        if concrete.is_none() && !aggregate_map {
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
                    concrete
                        .ok_or_else(|| Diagnostic::backend("represented map omitted its make"))?
                        .make()
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
            Ty::Map(key, element)
                if key.underlying() == &Ty::Int(IntTy::Int)
                    && element.underlying() == &Ty::String =>
            {
                hir::Builtin::MapI64GoStringClear
            }
            Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                hir::Builtin::SliceI64Clear
            }
            Ty::Slice(element) if element.underlying() == &Ty::String => {
                hir::Builtin::SliceGoStringClear
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
        let map = self.lower_expr(map, None)?;
        let representation = ConcreteMapRepresentation::for_ty(&map.ty).ok_or_else(|| {
            Diagnostic::unsupported(
                "delete requires a map with an executable key/value representation",
                source,
            )
        })?;
        let key = self.lower_expr(key, Some(&representation.key_ty()))?;
        let effects = map_effects(&[&map, &key], true, false, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(representation.delete()),
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
