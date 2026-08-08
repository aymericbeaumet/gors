//! Mechanical emission of the verified panic cleanup plan.

use std::collections::BTreeMap;

use proc_macro2::Span;

use super::{
    emit_pc_dispatch_from, emit_runtime_call, emit_statement, emit_terminator, slot_ident,
};
use crate::compiler::Diagnostic;
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::rust_ir;

pub(super) fn emit_body(
    function: &rust_ir::Function,
    function_paths: &BTreeMap<QualifiedDefId, syn::Path>,
    initializers: &[syn::Stmt],
    control_flow: &[syn::Stmt],
) -> Result<syn::Block, Diagnostic> {
    let cleanup = function
        .panic_cleanup
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("panic cleanup emission has no verified plan"))?;
    let cleanup_flow = emit_cleanup_dispatch(function, function_paths, cleanup)?;
    let active = slot_ident(cleanup.active);
    let recovered = slot_ident(cleanup.recovered);
    let payload: syn::Expr = syn::parse_quote! { __gors_panic_payload };
    let capture = emit_runtime_call(cleanup.capture.operation, vec![payload]);
    let recovered_payload: syn::Expr = syn::parse_quote! {
        #recovered.take().expect(
            "compiler rethrow of missing recovered panic value"
        )
    };
    let rethrow = emit_runtime_call(cleanup.rethrow.operation, vec![recovered_payload]);
    Ok(syn::parse_quote! {{
        #(#initializers)*
        let __gors_execution = ::std::panic::catch_unwind(
            ::std::panic::AssertUnwindSafe(|| {
                #(#control_flow)*
            })
        );
        match __gors_execution {
            ::std::result::Result::Ok(value) => value,
            ::std::result::Result::Err(__gors_panic_payload) => {
                #recovered = ::std::option::Option::Some(#capture);
                #active = ::std::option::Option::Some(true);
                let __gors_cleanup = ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(|| {
                        #(#cleanup_flow)*
                    })
                );
                match __gors_cleanup {
                    ::std::result::Result::Err(payload) => {
                        ::std::panic::resume_unwind(payload)
                    }
                    ::std::result::Result::Ok(value) => {
                        if *#active.as_ref().expect(
                            "compiler read of uninitialized panic recovery state"
                        ) {
                            #rethrow;
                            ::std::unreachable!(
                                "verified panic rethrow operation returned"
                            )
                        } else {
                            value
                        }
                    }
                }
            }
        }
    }})
}

fn emit_cleanup_dispatch(
    function: &rust_ir::Function,
    function_paths: &BTreeMap<QualifiedDefId, syn::Path>,
    cleanup: &rust_ir::PanicCleanup,
) -> Result<Vec<syn::Stmt>, Diagnostic> {
    let cleanup_pc = syn::Ident::new("__gors_cleanup_pc", Span::mixed_site());
    let mut arms = Vec::<syn::Arm>::new();
    for action in &cleanup.actions {
        let dispatch = syn::LitInt::new(&format!("{}u32", action.dispatch.0), Span::mixed_site());
        let registered = slot_ident(action.registered);
        let continuation = action.continuation.0;
        let replacement_target = action.replacement.target.0;
        let active = slot_ident(action.replacement.active);
        let recovered = slot_ident(action.replacement.recovered);
        let payload: syn::Expr = syn::parse_quote! { __gors_defer_panic_payload };
        let capture = emit_runtime_call(action.replacement.capture.operation, vec![payload]);
        let action_flow = emit_action_flow(function, function_paths, action)?;
        arms.push(syn::parse_quote! {
            #dispatch => {
                if *#registered.as_ref().expect(
                    "compiler read of uninitialized defer registration flag"
                ) {
                    let __gors_defer_execution = ::std::panic::catch_unwind(
                        ::std::panic::AssertUnwindSafe(|| {
                            #(#action_flow)*
                        })
                    );
                    match __gors_defer_execution {
                        ::std::result::Result::Ok(()) => {
                            #cleanup_pc = #continuation;
                        }
                        ::std::result::Result::Err(__gors_defer_panic_payload) => {
                            #recovered = ::std::option::Option::Some(#capture);
                            #active = ::std::option::Option::Some(true);
                            #cleanup_pc = #replacement_target;
                        }
                    }
                } else {
                    #cleanup_pc = #continuation;
                }
                continue;
            }
        });
    }

    let completion = syn::LitInt::new(&format!("{}u32", cleanup.completion.0), Span::mixed_site());
    let completion_flow = emit_pc_dispatch_from(function, function_paths, cleanup.completion.0)?;
    arms.push(syn::parse_quote! {
        #completion => {
            #(#completion_flow)*
        }
    });
    arms.push(syn::parse_quote! {
        _ => ::std::unreachable!("invalid verified panic cleanup block")
    });

    let entry = cleanup.entry.0;
    Ok(vec![
        syn::parse_quote! { let mut #cleanup_pc: u32 = #entry; },
        syn::parse_quote! {
            loop {
                match #cleanup_pc {
                    #(#arms),*
                }
            }
        },
    ])
}

fn emit_action_flow(
    function: &rust_ir::Function,
    function_paths: &BTreeMap<QualifiedDefId, syn::Path>,
    action: &rust_ir::DeferredAction,
) -> Result<Vec<syn::Stmt>, Diagnostic> {
    let pc = syn::Ident::new("__gors_defer_pc", Span::mixed_site());
    let mut arms = Vec::<syn::Arm>::new();
    for block_id in &action.blocks {
        let block = function.blocks.get(block_id.0 as usize).ok_or_else(|| {
            Diagnostic::backend(format!(
                "verified deferred action references missing block {}",
                block_id.0
            ))
        })?;
        let block_id = syn::LitInt::new(&format!("{}u32", block.id.0), Span::mixed_site());
        let statements = block
            .statements
            .iter()
            .map(|statement| emit_statement(statement, function))
            .collect::<Result<Vec<_>, _>>()?;
        let terminator = emit_terminator(&block.terminator, function, function_paths, &pc)?;
        arms.push(syn::parse_quote! {
            #block_id => {
                #(#statements)*
                #terminator
            }
        });
    }
    arms.push(syn::parse_quote! {
        _ => ::std::unreachable!("invalid verified deferred-action block")
    });

    let entry = action.entry.0;
    let continuation = action.continuation.0;
    Ok(vec![
        syn::parse_quote! { let mut #pc: u32 = #entry; },
        syn::parse_quote! {
            loop {
                if #pc == #continuation {
                    break;
                }
                match #pc {
                    #(#arms),*
                }
            }
        },
    ])
}
