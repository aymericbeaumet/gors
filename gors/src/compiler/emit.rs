//! Terminal Rust-IR-to-syntax emitter.
//!
//! This module is intentionally boring.  It does not infer semantic facts and
//! it does not run corrective `syn` passes. Rust representations and local-use
//! modes have already been selected and verified before this point.

use std::collections::BTreeMap;

use proc_macro2::Span;

use super::Diagnostic;
use super::rust_ir::{
    self, CallTarget, Constant, ControlFlowPlan, DefId, LocalId, Operand, Place, PrimitiveOp,
    ReadOp, RuntimeOp, RustLinkage, RustSymbol, RustType, Rvalue, RvalueKind, SlotInitialization,
    Statement, StorageClass, StoreOp, Terminator, TerminatorKind, ValueOp,
};

pub(super) fn emit_file(file: &rust_ir::File) -> Result<syn::File, Diagnostic> {
    let function_names = file
        .functions
        .iter()
        .map(|function| (function.id, function_ident(&function.artifact.symbol)))
        .collect::<BTreeMap<_, _>>();
    let mut items = Vec::new();
    for function in &file.functions {
        items.push(syn::Item::Fn(emit_function(function, &function_names)?));
    }
    Ok(syn::File {
        shebang: None,
        attrs: Vec::new(),
        items,
    })
}

fn emit_function(
    function: &rust_ir::Function,
    function_names: &BTreeMap<DefId, syn::Ident>,
) -> Result<syn::ItemFn, Diagnostic> {
    let name = function_names.get(&function.id).cloned().ok_or_else(|| {
        Diagnostic::backend(format!("missing Rust symbol for DefId {}", function.id))
    })?;
    let parameter_idents = function
        .parameters
        .iter()
        .enumerate()
        .map(|(index, _)| syn::Ident::new(&format!("__gors_arg_{index}"), Span::mixed_site()))
        .collect::<Vec<_>>();
    let parameters = function
        .parameters
        .iter()
        .zip(&parameter_idents)
        .map(|(local, ident)| {
            let ty = function
                .locals
                .get(local.0 as usize)
                .ok_or_else(|| Diagnostic::backend(format!("invalid parameter local {}", local.0)))?
                .ty;
            let ty = emit_type(&ty)?;
            Ok::<syn::FnArg, Diagnostic>(syn::parse_quote! { #ident: #ty })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let output = emit_return_type(&function.signature.results)?;

    let mut initializers = Vec::<syn::Stmt>::new();
    for local in &function.locals {
        let slot = slot_ident(local.id);
        let ty = emit_type(&local.ty)?;
        if local.storage != StorageClass::CheckedOptionSlot {
            return Err(Diagnostic::backend("unsupported verified storage plan"));
        }
        let initializer = match local.initialization {
            SlotInitialization::Parameter(index) => {
                Some(parameter_idents.get(index).cloned().ok_or_else(|| {
                    Diagnostic::backend(format!(
                        "missing emitted parameter identifier at index {index}"
                    ))
                })?)
            }
            SlotInitialization::Uninitialized => None,
        };
        let statement = if let Some(initializer) = initializer {
            syn::parse_quote! {
                let mut #slot: ::std::option::Option<#ty> = ::std::option::Option::Some(#initializer);
            }
        } else {
            syn::parse_quote! {
                let mut #slot: ::std::option::Option<#ty> = ::std::option::Option::None;
            }
        };
        initializers.push(statement);
    }

    let control_flow = match &function.control_flow {
        ControlFlowPlan::StructuredLinear { order } => {
            emit_structured_linear(function, function_names, order)?
        }
        ControlFlowPlan::PcDispatchU32 => emit_pc_dispatch(function, function_names)?,
    };

    let body: syn::Block = syn::parse_quote! {{
        #(#initializers)*
        #(#control_flow)*
    }};

    let visibility: syn::Visibility = match function.artifact.linkage {
        RustLinkage::Internal => syn::Visibility::Inherited,
        RustLinkage::Public => syn::parse_quote! { pub },
    };
    Ok(syn::ItemFn {
        attrs: Vec::new(),
        vis: visibility,
        sig: syn::Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: Default::default(),
            ident: name,
            generics: syn::Generics::default(),
            paren_token: Default::default(),
            inputs: parameters.into_iter().collect(),
            variadic: None,
            output,
        },
        block: Box::new(body),
    })
}

fn emit_structured_linear(
    function: &rust_ir::Function,
    function_names: &BTreeMap<DefId, syn::Ident>,
    order: &[rust_ir::BasicBlockId],
) -> Result<Vec<syn::Stmt>, Diagnostic> {
    let mut emitted = Vec::new();
    for block_id in order {
        let block = function
            .blocks
            .get(block_id.0 as usize)
            .ok_or_else(|| Diagnostic::backend("verified structured block is missing"))?;
        emitted.extend(
            block
                .statements
                .iter()
                .map(|statement| emit_statement(statement, function))
                .collect::<Result<Vec<_>, _>>()?,
        );
        match &block.terminator.kind {
            TerminatorKind::Goto(_) => {}
            TerminatorKind::Call {
                target,
                args,
                destination,
                ..
            } => {
                let args = args
                    .iter()
                    .map(|arg| emit_operand(arg, function))
                    .collect::<Result<Vec<_>, _>>()?;
                let call = emit_call(target, args, function_names)?;
                emitted.push(if let Some(destination) = destination {
                    let slot = slot_ident(destination.local);
                    syn::parse_quote! {
                        #slot = ::std::option::Option::Some(#call);
                    }
                } else {
                    syn::parse_quote! { #call; }
                });
            }
            TerminatorKind::Return(values) => {
                let values = values
                    .iter()
                    .map(|value| emit_operand(value, function))
                    .collect::<Result<Vec<_>, _>>()?;
                let value: syn::Expr = match values.as_slice() {
                    [] => syn::parse_quote! { () },
                    [value] => value.clone(),
                    values => syn::parse_quote! { (#(#values),*) },
                };
                emitted.push(syn::parse_quote! { return #value; });
            }
            TerminatorKind::SwitchBool { .. } | TerminatorKind::Unreachable => {
                return Err(Diagnostic::backend(
                    "verified structured-linear plan contains non-linear control flow",
                ));
            }
        }
    }
    Ok(emitted)
}

fn emit_pc_dispatch(
    function: &rust_ir::Function,
    function_names: &BTreeMap<DefId, syn::Ident>,
) -> Result<Vec<syn::Stmt>, Diagnostic> {
    let entry = function.entry.0;
    let pc = syn::Ident::new("__gors_pc", Span::mixed_site());
    let mut arms = Vec::<syn::Arm>::new();
    for block in &function.blocks {
        let block_id = syn::LitInt::new(&format!("{}u32", block.id.0), Span::mixed_site());
        let statements = block
            .statements
            .iter()
            .map(|statement| emit_statement(statement, function))
            .collect::<Result<Vec<_>, _>>()?;
        let terminator = emit_terminator(&block.terminator, function, function_names, &pc)?;
        arms.push(syn::parse_quote! {
            #block_id => {
                #(#statements)*
                #terminator
            }
        });
    }
    arms.push(syn::parse_quote! {
        _ => ::std::unreachable!("invalid compiler Rust IR block")
    });
    Ok(vec![
        syn::parse_quote! { let mut #pc: u32 = #entry; },
        syn::parse_quote! {
            loop {
                match #pc {
                    #(#arms),*
                }
            }
        },
    ])
}

fn emit_statement(
    statement: &Statement,
    function: &rust_ir::Function,
) -> Result<syn::Stmt, Diagnostic> {
    if statement.store != StoreOp::SetSome {
        return Err(Diagnostic::backend("unsupported verified store plan"));
    }
    let slot = slot_ident(statement.destination.local);
    let value = emit_rvalue(&statement.value, function)?;
    Ok(syn::parse_quote! {
        #slot = ::std::option::Option::Some(#value);
    })
}

fn emit_terminator(
    terminator: &Terminator,
    function: &rust_ir::Function,
    function_names: &BTreeMap<DefId, syn::Ident>,
    pc: &syn::Ident,
) -> Result<syn::Expr, Diagnostic> {
    match &terminator.kind {
        TerminatorKind::Goto(target) => {
            let target = target.0;
            Ok(syn::parse_quote! {{
                #pc = #target;
                continue;
            }})
        }
        TerminatorKind::SwitchBool {
            condition,
            then_target,
            else_target,
        } => {
            let condition = emit_operand(condition, function)?;
            let then_target = then_target.0;
            let else_target = else_target.0;
            Ok(syn::parse_quote! {{
                #pc = if #condition { #then_target } else { #else_target };
                continue;
            }})
        }
        TerminatorKind::Call {
            target: call_target,
            args,
            destination,
            next,
        } => {
            let emitted_args = args
                .iter()
                .map(|arg| emit_operand(arg, function))
                .collect::<Result<Vec<_>, _>>()?;
            let call = emit_call(call_target, emitted_args, function_names)?;
            let target = next.0;
            if let Some(destination) = destination {
                let slot = slot_ident(destination.local);
                Ok(syn::parse_quote! {{
                    #slot = ::std::option::Option::Some(#call);
                    #pc = #target;
                    continue;
                }})
            } else {
                Ok(syn::parse_quote! {{
                    #call;
                    #pc = #target;
                    continue;
                }})
            }
        }
        TerminatorKind::Return(values) => {
            let values = values
                .iter()
                .map(|value| emit_operand(value, function))
                .collect::<Result<Vec<_>, _>>()?;
            let value: syn::Expr = match values.as_slice() {
                [] => syn::parse_quote! { () },
                [value] => value.clone(),
                values => syn::parse_quote! { (#(#values),*) },
            };
            Ok(syn::parse_quote! { return #value })
        }
        TerminatorKind::Unreachable => Ok(syn::parse_quote! {
            ::std::unreachable!("entered unreachable compiler Rust IR block")
        }),
    }
}

fn emit_call(
    target: &CallTarget,
    args: Vec<syn::Expr>,
    function_names: &BTreeMap<DefId, syn::Ident>,
) -> Result<syn::Expr, Diagnostic> {
    match target {
        CallTarget::Function(id) => {
            let function = function_names
                .get(id)
                .ok_or_else(|| Diagnostic::backend(format!("missing callee DefId {id}")))?;
            Ok(syn::parse_quote! { #function(#(#args),*) })
        }
        CallTarget::Runtime(operation) => Ok(emit_runtime_call(*operation, args)),
    }
}

fn emit_rvalue(rvalue: &Rvalue, function: &rust_ir::Function) -> Result<syn::Expr, Diagnostic> {
    match &rvalue.kind {
        RvalueKind::Use(operand) => emit_operand(operand, function),
        RvalueKind::Unary { op, operand } => {
            let operand = emit_operand(operand, function)?;
            emit_value_op(*op, vec![operand])
        }
        RvalueKind::Binary { op, left, right } => {
            let left = emit_operand(left, function)?;
            let right = emit_operand(right, function)?;
            emit_value_op(*op, vec![left, right])
        }
    }
}

fn emit_value_op(operation: ValueOp, args: Vec<syn::Expr>) -> Result<syn::Expr, Diagnostic> {
    match operation {
        ValueOp::Primitive(operation) => emit_primitive_op(operation, &args),
        ValueOp::Runtime(operation) => Ok(emit_runtime_call(operation, args)),
    }
}

fn emit_primitive_op(operation: PrimitiveOp, args: &[syn::Expr]) -> Result<syn::Expr, Diagnostic> {
    use PrimitiveOp::*;
    let expression = match (operation, args) {
        (BoolNot | IntBitNot, [value]) => syn::parse_quote! { !(#value) },
        (IntWrappingNeg, [value]) => syn::parse_quote! { (#value).wrapping_neg() },
        (FloatNeg, [value]) => syn::parse_quote! { -(#value) },
        (ComplexNeg, [value]) => syn::parse_quote! { [-(#value)[0], -(#value)[1]] },
        (IntBitAnd, [left, right]) => syn::parse_quote! { (#left) & (#right) },
        (IntBitOr, [left, right]) => syn::parse_quote! { (#left) | (#right) },
        (IntBitXor, [left, right]) => syn::parse_quote! { (#left) ^ (#right) },
        (IntAndNot, [left, right]) => syn::parse_quote! { (#left) & !(#right) },
        (IntWrappingAdd, [left, right]) => {
            syn::parse_quote! { (#left).wrapping_add(#right) }
        }
        (IntWrappingSub, [left, right]) => {
            syn::parse_quote! { (#left).wrapping_sub(#right) }
        }
        (IntWrappingMul, [left, right]) => {
            syn::parse_quote! { (#left).wrapping_mul(#right) }
        }
        (FloatAdd, [left, right]) => syn::parse_quote! { (#left) + (#right) },
        (FloatSub, [left, right]) => syn::parse_quote! { (#left) - (#right) },
        (FloatMul, [left, right]) => syn::parse_quote! { (#left) * (#right) },
        (FloatDiv, [left, right]) => syn::parse_quote! { (#left) / (#right) },
        (ComplexAdd, [left, right]) => {
            syn::parse_quote! { [(#left)[0] + (#right)[0], (#left)[1] + (#right)[1]] }
        }
        (ComplexSub, [left, right]) => {
            syn::parse_quote! { [(#left)[0] - (#right)[0], (#left)[1] - (#right)[1]] }
        }
        (ComplexMul, [left, right]) => syn::parse_quote! {
            [
                (#left)[0] * (#right)[0] - (#left)[1] * (#right)[1],
                (#left)[0] * (#right)[1] + (#left)[1] * (#right)[0],
            ]
        },
        (ComplexDiv, [left, right]) => syn::parse_quote! {
            {
                let __gors_denominator = (#right)[0] * (#right)[0] + (#right)[1] * (#right)[1];
                [
                    ((#left)[0] * (#right)[0] + (#left)[1] * (#right)[1]) / __gors_denominator,
                    ((#left)[1] * (#right)[0] - (#left)[0] * (#right)[1]) / __gors_denominator,
                ]
            }
        },
        (BoolEqual | IntEqual | FloatEqual | StringEqual, [left, right]) => {
            syn::parse_quote! { (#left) == (#right) }
        }
        (BoolNotEqual | IntNotEqual | FloatNotEqual | StringNotEqual, [left, right]) => {
            syn::parse_quote! { (#left) != (#right) }
        }
        (ComplexEqual, [left, right]) => syn::parse_quote! {
            (#left)[0] == (#right)[0] && (#left)[1] == (#right)[1]
        },
        (ComplexNotEqual, [left, right]) => syn::parse_quote! {
            (#left)[0] != (#right)[0] || (#left)[1] != (#right)[1]
        },
        (IntLess | FloatLess | StringLess, [left, right]) => {
            syn::parse_quote! { (#left) < (#right) }
        }
        (IntLessEqual | FloatLessEqual | StringLessEqual, [left, right]) => {
            syn::parse_quote! { (#left) <= (#right) }
        }
        (IntGreater | FloatGreater | StringGreater, [left, right]) => {
            syn::parse_quote! { (#left) > (#right) }
        }
        (IntGreaterEqual | FloatGreaterEqual | StringGreaterEqual, [left, right]) => {
            syn::parse_quote! { (#left) >= (#right) }
        }
        (operation, _) => {
            return Err(Diagnostic::backend(format!(
                "invalid verified primitive operation arity for {operation:?}"
            )));
        }
    };
    Ok(expression)
}

fn emit_runtime_call(operation: RuntimeOp, args: Vec<syn::Expr>) -> syn::Expr {
    let runtime_crate = syn::Ident::new(crate::artifact::RUNTIME_CRATE_NAME, Span::mixed_site());
    let symbol = syn::Ident::new(operation.symbol(), Span::mixed_site());
    syn::parse_quote! { ::#runtime_crate::#symbol(#(#args),*) }
}

fn emit_operand(operand: &Operand, function: &rust_ir::Function) -> Result<syn::Expr, Diagnostic> {
    match operand {
        Operand::Read {
            place,
            op: ReadOp::ProvenInitializedCopy,
        } => {
            let slot = checked_slot(*place, function)?;
            Ok(syn::parse_quote! {
                *#slot.as_ref().expect("compiler read of uninitialized Go local")
            })
        }
        Operand::Read {
            place,
            op: ReadOp::ProvenInitializedClone,
        } => {
            let slot = checked_slot(*place, function)?;
            Ok(syn::parse_quote! {
                #slot.as_ref().expect("compiler read of uninitialized Go local").clone()
            })
        }
        Operand::Read {
            place,
            op: ReadOp::ProvenLastUseMove,
        } => {
            let slot = checked_slot(*place, function)?;
            Ok(syn::parse_quote! {
                #slot.take().expect("compiler move of uninitialized Go local")
            })
        }
        Operand::Constant(value) => emit_constant(value),
        Operand::Unit => Ok(syn::parse_quote! { () }),
    }
}

fn checked_slot(place: Place, function: &rust_ir::Function) -> Result<syn::Ident, Diagnostic> {
    function
        .locals
        .get(place.local.0 as usize)
        .ok_or_else(|| Diagnostic::backend(format!("invalid emitted local {}", place.local.0)))?;
    Ok(slot_ident(place.local))
}

fn emit_constant(value: &Constant) -> Result<syn::Expr, Diagnostic> {
    match value {
        Constant::Bool(value) => Ok(if *value {
            syn::parse_quote! { true }
        } else {
            syn::parse_quote! { false }
        }),
        Constant::I64(value) => {
            if *value == i64::MIN {
                Ok(syn::parse_quote! { ::std::primitive::i64::MIN })
            } else {
                let magnitude = value.unsigned_abs();
                let literal = syn::LitInt::new(&format!("{magnitude}i64"), Span::mixed_site());
                Ok(if *value < 0 {
                    syn::parse_quote! { -(#literal) }
                } else {
                    syn::parse_quote! { #literal }
                })
            }
        }
        Constant::F64(bits) => {
            let bits = syn::LitInt::new(&format!("{bits}u64"), Span::mixed_site());
            Ok(syn::parse_quote! { ::std::primitive::f64::from_bits(#bits) })
        }
        Constant::Complex128 { real, imag } => {
            let real = syn::LitInt::new(&format!("{real}u64"), Span::mixed_site());
            let imag = syn::LitInt::new(&format!("{imag}u64"), Span::mixed_site());
            Ok(syn::parse_quote! {
                [
                    ::std::primitive::f64::from_bits(#real),
                    ::std::primitive::f64::from_bits(#imag),
                ]
            })
        }
        Constant::RuntimeStaticBytes { op, bytes } => {
            let bytes = syn::LitByteStr::new(bytes, Span::mixed_site());
            Ok(emit_runtime_call(*op, vec![syn::parse_quote! { #bytes }]))
        }
        Constant::RuntimeStaticI64s { op, values } => {
            let values = values
                .iter()
                .map(|value| emit_constant(&Constant::I64(*value)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(emit_runtime_call(
                *op,
                vec![syn::parse_quote! { &[#(#values),*] }],
            ))
        }
    }
}

fn emit_return_type(results: &[RustType]) -> Result<syn::ReturnType, Diagnostic> {
    let ty = match results {
        [] => return Ok(syn::ReturnType::Default),
        [single] => emit_type(single)?,
        many => {
            let elements = many.iter().map(emit_type).collect::<Result<Vec<_>, _>>()?;
            syn::parse_quote! { (#(#elements),*) }
        }
    };
    Ok(syn::ReturnType::Type(Default::default(), Box::new(ty)))
}

fn emit_type(ty: &RustType) -> Result<syn::Type, Diagnostic> {
    let runtime_crate = syn::Ident::new(crate::artifact::RUNTIME_CRATE_NAME, Span::mixed_site());
    Ok(match ty {
        RustType::Unit => syn::parse_quote! { () },
        RustType::Bool => syn::parse_quote! { bool },
        RustType::GoString => syn::parse_quote! { ::#runtime_crate::GoString },
        RustType::I64 => syn::parse_quote! { i64 },
        RustType::F64 => syn::parse_quote! { f64 },
        RustType::Complex128 => syn::parse_quote! { [f64; 2] },
        RustType::GoSliceI64 => syn::parse_quote! { ::#runtime_crate::GoSliceI64 },
    })
}

fn slot_ident(local: LocalId) -> syn::Ident {
    syn::Ident::new(&format!("__gors_l{}", local.0), Span::mixed_site())
}

fn function_ident(symbol: &RustSymbol) -> syn::Ident {
    syn::Ident::new(symbol.as_str(), Span::mixed_site())
}
