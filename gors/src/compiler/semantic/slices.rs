//! Built-in slice calls and proof-backed function capture representation.

use super::FunctionLowerer;
use super::expression_lower::{constant_index_value, slice_runtime_effects};
use super::expressions::coerce_expr;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::ExprSyntax;
use crate::compiler::types::{IntTy, Ty, UintTy, UntypedTy};

impl FunctionLowerer {
    pub(super) fn lower_slice_builtin_call(
        &mut self,
        name: &str,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let slice_ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
        let byte_slice_ty = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        let (builtin, args, ty, allocates, writes, panics) = match name {
            "make" => {
                let (declared_syntax, len_syntax, cap_syntax) = match arguments {
                    [declared, len] => (declared, len, None),
                    [declared, len, cap] => (declared, len, Some(cap)),
                    _ => {
                        return Err(Diagnostic::semantic(
                            "make(slice, len[, cap]) requires two or three arguments",
                            source,
                        ));
                    }
                };
                let declared = self.lower_scoped_type(declared_syntax, source)?;
                let builtin = if declared == slice_ty {
                    hir::Builtin::SliceI64Make
                } else if declared == byte_slice_ty {
                    hir::Builtin::SliceU8Make
                } else if matches!(declared.underlying(), Ty::Slice(element) if element.underlying() == &Ty::String)
                {
                    hir::Builtin::SliceGoStringMake
                } else {
                    return Err(Diagnostic::unsupported(
                        "make currently supports []int, []byte, and string-element slice values",
                        source,
                    ));
                };
                let len = self.lower_expr(len_syntax, Some(&Ty::Int(IntTy::Int)))?;
                let len_constant = constant_index_value(&len);
                if let Some(len_value) = len_constant
                    && len_value < 0
                {
                    return Err(Diagnostic::semantic(
                        format!(
                            "invalid argument: index {len_value} (constant of type int) must not be negative"
                        ),
                        source,
                    ));
                }
                let cap = if let Some(cap) = cap_syntax {
                    let cap = self.lower_expr(cap, Some(&Ty::Int(IntTy::Int)))?;
                    if let Some(cap_value) = constant_index_value(&cap) {
                        if cap_value < 0 {
                            return Err(Diagnostic::semantic(
                                format!(
                                    "invalid argument: index {cap_value} (constant of type int) must not be negative"
                                ),
                                source,
                            ));
                        }
                        if len_constant.is_some_and(|len_value| len_value > cap_value) {
                            return Err(Diagnostic::semantic(
                                "invalid argument: length and capacity swapped",
                                source,
                            ));
                        }
                    }
                    cap
                } else {
                    self.lower_optional_slice_bound(None, declared_syntax.source)?
                };
                (builtin, vec![len, cap], declared, true, false, true)
            }
            "cap" => {
                let [value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "cap requires exactly one argument",
                        source,
                    ));
                };
                let value = self.lower_expr(value, None)?;
                let builtin = match value.ty.underlying() {
                    Ty::Slice(element)
                        if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
                    {
                        hir::Builtin::SliceI64Cap
                    }
                    Ty::Slice(element) if element.underlying() == &Ty::String => {
                        hir::Builtin::SliceGoStringCap
                    }
                    ty => {
                        return Err(Diagnostic::unsupported(
                            format!("cap is not yet implemented for {ty:?}"),
                            source,
                        ));
                    }
                };
                (
                    builtin,
                    vec![value],
                    Ty::Int(IntTy::Int),
                    false,
                    false,
                    false,
                )
            }
            "append" => {
                let [slice, value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "append currently requires one slice and one element",
                        source,
                    ));
                };
                if spread {
                    let slice = self.lower_expr(slice, Some(&byte_slice_ty))?;
                    let mut value = self.lower_expr(value, None)?;
                    if value.ty == Ty::Untyped(UntypedTy::String) {
                        coerce_expr(&mut value, &Ty::String, source)?;
                    }
                    let builtin = match value.ty {
                        Ty::String => hir::Builtin::SliceU8AppendString,
                        ref ty if ty == &byte_slice_ty => hir::Builtin::SliceU8AppendSlice,
                        _ => {
                            return Err(Diagnostic::semantic(
                                "[]byte append spread requires a string or []byte source",
                                source,
                            ));
                        }
                    };
                    (
                        builtin,
                        vec![slice, value],
                        byte_slice_ty,
                        true,
                        true,
                        false,
                    )
                } else {
                    let slice = self.lower_expr(slice, None)?;
                    let Ty::Slice(element) = slice.ty.underlying() else {
                        return Err(Diagnostic::semantic(
                            "append requires a slice as its first argument",
                            source,
                        ));
                    };
                    if element.snapshot_function_result().is_some() {
                        let element_ty = element.as_ref().clone();
                        let result_ty = slice.ty.clone();
                        let capture =
                            self.lower_snapshot_function_capture(value, &element_ty, source)?;
                        return self.finish_slice_builtin_call(
                            hir::Builtin::SnapshotFunctionSliceAppend,
                            vec![slice, capture],
                            result_ty,
                            true,
                            true,
                            false,
                            node,
                            source,
                            expected,
                        );
                    }
                    if !matches!(
                        element.underlying(),
                        Ty::Int(IntTy::Int | IntTy::Int32) | Ty::String
                    ) {
                        return Err(Diagnostic::unsupported(
                            "non-spread append currently supports int, rune, string, and proven function slices",
                            source,
                        ));
                    }
                    let element_ty = element.as_ref().clone();
                    let result_ty = slice.ty.clone();
                    let value = self.lower_expr(value, Some(&element_ty))?;
                    let builtin = if element.underlying() == &Ty::String {
                        hir::Builtin::SliceGoStringAppend
                    } else {
                        hir::Builtin::SliceI64Append
                    };
                    (builtin, vec![slice, value], result_ty, true, true, false)
                }
            }
            "copy" => {
                let [destination, source_value] = arguments else {
                    return Err(Diagnostic::semantic(
                        "copy requires a destination and source",
                        source,
                    ));
                };
                if spread {
                    return Err(Diagnostic::semantic("copy does not accept ...", source));
                }
                let destination = self.lower_expr(destination, None)?;
                let (builtin, source_value) = if destination.ty == slice_ty {
                    (
                        hir::Builtin::SliceI64Copy,
                        self.lower_expr(source_value, Some(&slice_ty))?,
                    )
                } else if destination.ty == byte_slice_ty {
                    let mut source_value = self.lower_expr(source_value, None)?;
                    if source_value.ty == Ty::Untyped(UntypedTy::String) {
                        coerce_expr(&mut source_value, &Ty::String, source)?;
                    }
                    let builtin = if source_value.ty == byte_slice_ty {
                        hir::Builtin::SliceU8Copy
                    } else if source_value.ty == Ty::String {
                        hir::Builtin::SliceU8CopyString
                    } else {
                        return Err(Diagnostic::semantic(
                            "copy with a []byte destination requires a []byte or string source",
                            source,
                        ));
                    };
                    (builtin, source_value)
                } else if matches!(destination.ty.underlying(), Ty::Slice(element) if element.underlying() == &Ty::String)
                {
                    let source_value = self.lower_expr(source_value, None)?;
                    let (Ty::Slice(destination_element), Ty::Slice(source_element)) =
                        (destination.ty.underlying(), source_value.ty.underlying())
                    else {
                        return Err(Diagnostic::semantic(
                            "copy with a string-element slice destination requires a slice source",
                            source,
                        ));
                    };
                    if destination_element != source_element {
                        return Err(Diagnostic::semantic(
                            "copy requires source and destination slices with identical element types",
                            source,
                        ));
                    }
                    (hir::Builtin::SliceGoStringCopy, source_value)
                } else {
                    return Err(Diagnostic::semantic(
                        "copy currently supports []int, []byte, or identical string-element slice pairs, plus a []byte destination and string source",
                        source,
                    ));
                };
                (
                    builtin,
                    vec![destination, source_value],
                    Ty::Int(IntTy::Int),
                    false,
                    true,
                    false,
                )
            }
            _ => {
                return Err(Diagnostic::backend(
                    "non-slice builtin reached slice lowering",
                ));
            }
        };
        self.finish_slice_builtin_call(
            builtin, args, ty, allocates, writes, panics, node, source, expected,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_slice_builtin_call(
        &mut self,
        builtin: hir::Builtin,
        args: Vec<hir::Expr>,
        ty: Ty,
        allocates: bool,
        writes: bool,
        panics: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let argument_refs = args.iter().collect::<Vec<_>>();
        let effects = slice_runtime_effects(&argument_refs, writes, allocates, panics);
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }
}
