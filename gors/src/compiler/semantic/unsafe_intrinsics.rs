//! Typed lowering for the compiler-defined `unsafe` package.

use super::FunctionLowerer;
use super::expression_lower::slice_runtime_effects;
use super::expressions::coerce_expr;
use super::pointers::pointer_effects;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, IdentSyntax};
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Ty, UintTy};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn try_lower_unsafe_pointer_roundtrip(
        &mut self,
        expression: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<Option<hir::Expr>, Diagnostic> {
        let ExprSyntaxKind::Call {
            callee,
            arguments,
            spread: false,
        } = &expression.kind
        else {
            return Ok(None);
        };
        let [erased_pointer] = arguments.as_ref() else {
            return Ok(None);
        };
        let Some(addressed) = unsafe_pointer_address(erased_pointer, &self.intrinsic_packages)
        else {
            return Ok(None);
        };
        let target = super::lower_type(callee, &self.type_aliases, source)?;
        let Ty::Pointer(element) = target.underlying() else {
            return Err(Diagnostic::semantic(
                "unsafe.Pointer round trip requires conversion to a pointer type",
                source,
            ));
        };
        let mut result = self.lower_expr(addressed, Some(element))?;
        result.node = node;
        result.source = source;
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(Some(result))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_unsafe_intrinsic_call(
        &mut self,
        package: &str,
        member: &IdentSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic(
                "unsafe package intrinsics do not accept ...",
                source,
            ));
        }
        match member.name.as_ref() {
            "Sizeof" | "Alignof" => {
                let [argument] = arguments else {
                    return Err(Diagnostic::semantic(
                        format!("unsafe.{} requires exactly one argument", member.name),
                        source,
                    ));
                };
                let argument = self.lower_expr(argument, None)?;
                let layout = type_layout(&argument.ty).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!(
                            "unsafe.{} is not implemented for {:?}",
                            member.name, argument.ty
                        ),
                        source,
                    )
                })?;
                let value = if member.name.as_ref() == "Sizeof" {
                    layout.size
                } else {
                    layout.align
                };
                uintptr_constant(node, source, value, expected)
            }
            "Offsetof" => {
                let [argument] = arguments else {
                    return Err(Diagnostic::semantic(
                        "unsafe.Offsetof requires exactly one selector argument",
                        source,
                    ));
                };
                let value = self.unsafe_offsetof(argument, source)?;
                uintptr_constant(node, source, value, expected)
            }
            "String" => self.lower_unsafe_string(package, arguments, node, source, expected),
            "Pointer" => self.lower_unsafe_pointer(arguments, node, source, expected),
            "SliceData" => Err(Diagnostic::unsupported(
                "unsafe.SliceData pointers are supported when consumed by unsafe.String",
                source,
            )),
            name => Err(Diagnostic::unsupported(
                format!("unsafe.{name} is not yet implemented"),
                source,
            )),
        }
    }

    fn lower_unsafe_pointer(
        &mut self,
        arguments: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [argument] = arguments else {
            return Err(Diagnostic::semantic(
                "unsafe.Pointer requires exactly one pointer argument",
                source,
            ));
        };
        let ExprSyntaxKind::Unary {
            token: Token::AND,
            expression: addressed,
        } = &argument.kind
        else {
            return Err(Diagnostic::unsupported(
                "unsafe.Pointer currently requires the address of an executable value",
                source,
            ));
        };
        let value = self.lower_expr(addressed, None)?;
        if value.ty.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                format!(
                    "unsafe.Pointer address conversion is not implemented for {:?}",
                    value.ty
                ),
                source,
            ));
        }
        let ty = Ty::Pointer(Box::new(value.ty.clone()));
        let effects = pointer_effects(&[&value], true, true, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::AddressOfValue(Box::new(value)),
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

    fn unsafe_offsetof(
        &mut self,
        argument: &ExprSyntax,
        source: SourceRef,
    ) -> Result<u64, Diagnostic> {
        let ExprSyntaxKind::Selector { base, member } = &argument.kind else {
            return Err(Diagnostic::semantic(
                "unsafe.Offsetof requires a struct field selector",
                source,
            ));
        };
        let structure = self.lower_expr(base, None)?;
        let Ty::Struct(fields) = structure.ty.underlying() else {
            return Err(Diagnostic::semantic(
                "unsafe.Offsetof requires a struct field selector",
                source,
            ));
        };
        let mut offset = 0_u64;
        for field in fields {
            let layout = type_layout(&field.ty).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!(
                        "unsafe.Offsetof is not implemented for field type {:?}",
                        field.ty
                    ),
                    source,
                )
            })?;
            offset = align_up(offset, layout.align)?;
            if field.name == member.name.as_ref() {
                return Ok(offset);
            }
            offset = offset.checked_add(layout.size).ok_or_else(|| {
                Diagnostic::semantic("struct layout exceeds the target address space", source)
            })?;
        }
        Err(Diagnostic::semantic(
            format!("struct has no field {}", member.name),
            source,
        ))
    }

    fn lower_unsafe_string(
        &mut self,
        package: &str,
        arguments: &[ExprSyntax],
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let [pointer, length] = arguments else {
            return Err(Diagnostic::semantic(
                "unsafe.String requires a byte pointer and length",
                source,
            ));
        };
        let Some(slice_syntax) = slice_data_argument(pointer, package) else {
            return Err(Diagnostic::unsupported(
                "unsafe.String currently requires a pointer from unsafe.SliceData",
                source,
            ));
        };
        let slice = self.lower_expr(slice_syntax, None)?;
        let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        if slice.ty.underlying() != byte_slice.underlying() {
            return Err(Diagnostic::semantic(
                "unsafe.SliceData requires a []byte value",
                source,
            ));
        }
        let length = self.lower_expr(length, Some(&Ty::Int(IntTy::Int)))?;
        let low = self.integer_constant(pointer, "0")?;
        let max = self.integer_constant(pointer, "-1")?;
        let range_node = self.alloc_node(pointer.source)?;
        let range_source = SourceRef::node(range_node);
        let range_effects =
            slice_runtime_effects(&[&slice, &low, &length, &max], false, false, true);
        let range = hir::Expr {
            node: range_node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::SliceU8Range),
                args: vec![slice, low, length, max],
            },
            ty: byte_slice,
            category: hir::ValueCategory::Value,
            effects: range_effects,
            source: range_source,
        };
        let effects = slice_runtime_effects(&[&range], false, true, false);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceU8),
                args: vec![range],
            },
            ty: Ty::String,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    fn integer_constant(
        &mut self,
        expression: &ExprSyntax,
        value: &str,
    ) -> Result<hir::Expr, Diagnostic> {
        let node = self.alloc_node(expression.source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Constant(ConstValue::Int(value.to_string())),
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(node),
        })
    }
}

fn unsafe_pointer_address<'a>(
    expression: &'a ExprSyntax,
    intrinsic_packages: &std::collections::BTreeSet<String>,
) -> Option<&'a ExprSyntax> {
    let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread: false,
    } = &expression.kind
    else {
        return None;
    };
    let ExprSyntaxKind::Selector { base, member } = &callee.kind else {
        return None;
    };
    let ExprSyntaxKind::Ident(package) = &base.kind else {
        return None;
    };
    let [argument] = arguments.as_ref() else {
        return None;
    };
    let ExprSyntaxKind::Unary {
        token: Token::AND,
        expression: addressed,
    } = &argument.kind
    else {
        return None;
    };
    (intrinsic_packages.contains(package.name.as_ref()) && member.name.as_ref() == "Pointer")
        .then_some(addressed.as_ref())
}

fn slice_data_argument<'a>(expression: &'a ExprSyntax, package: &str) -> Option<&'a ExprSyntax> {
    let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread: false,
    } = &expression.kind
    else {
        return None;
    };
    let ExprSyntaxKind::Selector { base, member } = &callee.kind else {
        return None;
    };
    let ExprSyntaxKind::Ident(base) = &base.kind else {
        return None;
    };
    (base.name.as_ref() == package && member.name.as_ref() == "SliceData")
        .then(|| arguments.first())
        .flatten()
        .filter(|_| arguments.len() == 1)
}

fn uintptr_constant(
    node: NodeId,
    source: SourceRef,
    value: u64,
    expected: Option<&Ty>,
) -> Result<hir::Expr, Diagnostic> {
    let mut result = hir::Expr {
        node,
        kind: hir::ExprKind::Constant(ConstValue::Int(value.to_string())),
        ty: Ty::Uint(UintTy::Uintptr),
        category: hir::ValueCategory::Constant,
        effects: hir::Effects::default(),
        source,
    };
    if let Some(expected) = expected {
        coerce_expr(&mut result, expected, source)?;
    }
    Ok(result)
}

#[derive(Clone, Copy)]
struct Layout {
    size: u64,
    align: u64,
}

fn type_layout(ty: &Ty) -> Option<Layout> {
    let layout = match ty.underlying() {
        Ty::Unit => Layout { size: 0, align: 1 },
        Ty::Bool | Ty::Int(IntTy::Int8) | Ty::Uint(UintTy::Uint8) => Layout { size: 1, align: 1 },
        Ty::Int(IntTy::Int16) | Ty::Uint(UintTy::Uint16) => Layout { size: 2, align: 2 },
        Ty::Int(IntTy::Int32) | Ty::Uint(UintTy::Uint32) | Ty::Float(FloatTy::Float32) => {
            Layout { size: 4, align: 4 }
        }
        Ty::Int(IntTy::Int | IntTy::Int64)
        | Ty::Uint(UintTy::Uint | UintTy::Uint64 | UintTy::Uintptr)
        | Ty::Float(FloatTy::Float64)
        | Ty::Pointer(_)
        | Ty::Map(_, _)
        | Ty::Channel(_, _)
        | Ty::Function(_) => Layout { size: 8, align: 8 },
        Ty::Complex(ComplexTy::Complex64) => Layout { size: 8, align: 4 },
        Ty::Complex(ComplexTy::Complex128) | Ty::String | Ty::Interface(_) => {
            Layout { size: 16, align: 8 }
        }
        Ty::Slice(_) => Layout { size: 24, align: 8 },
        Ty::Array(length, element) => {
            let element = type_layout(element)?;
            Layout {
                size: element.size.checked_mul(*length)?,
                align: element.align,
            }
        }
        Ty::Struct(fields) => {
            let mut size = 0_u64;
            let mut alignment = 1_u64;
            for field in fields {
                let field = type_layout(&field.ty)?;
                alignment = alignment.max(field.align);
                size = align_up(size, field.align).ok()?;
                size = size.checked_add(field.size)?;
            }
            Layout {
                size: align_up(size, alignment).ok()?,
                align: alignment,
            }
        }
        Ty::Tuple(_) | Ty::Untyped(_) | Ty::Named { .. } | Ty::LocalNamed { .. } => return None,
    };
    Some(layout)
}

fn align_up(value: u64, alignment: u64) -> Result<u64, Diagnostic> {
    let remainder = value % alignment;
    if remainder == 0 {
        return Ok(value);
    }
    value
        .checked_add(alignment - remainder)
        .ok_or_else(|| Diagnostic::backend("target layout overflow"))
}
