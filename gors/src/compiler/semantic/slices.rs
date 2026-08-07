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
                return self.lower_append_builtin_call(arguments, spread, node, source, expected);
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

    fn lower_append_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Some((slice_syntax, argument_syntax)) = arguments.split_first() else {
            return Err(Diagnostic::semantic(
                "append requires a slice as its first argument",
                source,
            ));
        };
        let slice = self.lower_expr(slice_syntax, None)?;
        let Ty::Slice(element) = slice.ty.underlying() else {
            return Err(Diagnostic::semantic(
                "append requires a slice as its first argument",
                source,
            ));
        };
        let element_ty = element.as_ref().clone();
        let canonical_byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        let arguments = if spread {
            let [value_syntax] = argument_syntax else {
                return Err(Diagnostic::semantic(
                    "append with ... requires exactly one source value",
                    source,
                ));
            };
            let mut value = if self.is_predeclared_nil_identifier(value_syntax) {
                self.lower_expr(value_syntax, Some(&slice.ty))?
            } else {
                self.lower_expr(value_syntax, None)?
            };
            if element.underlying() == &Ty::Uint(UintTy::Uint8)
                && value.ty == Ty::Untyped(UntypedTy::String)
            {
                coerce_expr(&mut value, &Ty::String, source)?;
            }
            let variadic_slice_ty = Ty::Slice(Box::new(element_ty.clone()));
            let valid = if super::expressions::is_assignable(&slice.ty, &canonical_byte_slice)
                && value.ty.underlying() == &Ty::String
            {
                true
            } else {
                super::expressions::is_assignable(&value.ty, &variadic_slice_ty)
            };
            if !valid {
                return Err(Diagnostic::semantic(
                    "append spread source does not match the destination element type",
                    source,
                ));
            }
            if value.ty.underlying() != &Ty::String {
                coerce_expr(&mut value, &variadic_slice_ty, source)?;
            }
            if !matches!(
                element.underlying(),
                Ty::Int(IntTy::Int | IntTy::Int32) | Ty::Uint(UintTy::Uint8) | Ty::Interface(_)
            ) {
                return Err(Diagnostic::unsupported(
                    "spread append currently supports integer, byte, and interface slices",
                    source,
                ));
            }
            hir::AppendArguments::Spread(Box::new(value))
        } else {
            if argument_syntax.is_empty() {
                hir::AppendArguments::Elements(Vec::new())
            } else {
                if element.snapshot_function_result().is_some() && argument_syntax.len() > 1 {
                    return Err(Diagnostic::unsupported(
                        "function-slice append currently supports one element",
                        source,
                    ));
                }
                if element.underlying() == &Ty::String && argument_syntax.len() > 1 {
                    return Err(Diagnostic::unsupported(
                        "string-slice append currently supports one element",
                        source,
                    ));
                }
                if !matches!(
                    element.underlying(),
                    Ty::Int(IntTy::Int | IntTy::Int32)
                        | Ty::Uint(UintTy::Uint8)
                        | Ty::String
                        | Ty::Interface(_)
                ) && element.snapshot_function_result().is_none()
                {
                    return Err(Diagnostic::unsupported(
                        "append element type has no executable representation",
                        source,
                    ));
                }
                let mut values = Vec::with_capacity(argument_syntax.len());
                for value in argument_syntax {
                    let lowered = if element.snapshot_function_result().is_some() {
                        self.lower_snapshot_function_capture(value, &element_ty, source)?
                    } else {
                        self.lower_expr(value, Some(&element_ty))?
                    };
                    values.push(lowered);
                }
                hir::AppendArguments::Elements(values)
            }
        };
        let has_runtime_append = match &arguments {
            hir::AppendArguments::Elements(values) => !values.is_empty(),
            hir::AppendArguments::Spread(_) => true,
        };
        let mut effects = slice.effects;
        match &arguments {
            hir::AppendArguments::Elements(values) => {
                for value in values {
                    effects = effects.union(value.effects);
                }
            }
            hir::AppendArguments::Spread(value) => effects = effects.union(value.effects),
        }
        if has_runtime_append {
            effects = effects.union(hir::Effects {
                may_call: true,
                may_allocate: true,
                may_write: true,
                ..hir::Effects::default()
            });
        }
        let result_ty = slice.ty.clone();
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::Append {
                slice: Box::new(slice),
                arguments,
            },
            ty: result_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
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
