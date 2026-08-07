//! Mechanical emission of the verified panic cleanup plan.

use std::collections::BTreeMap;

use super::{emit_pc_dispatch_from, emit_runtime_call, slot_ident};
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
        .ok_or_else(|| Diagnostic::backend("panic cleanup emission has no verified plan"))?;
    let cleanup_flow = emit_pc_dispatch_from(function, function_paths, cleanup.entry.0)?;
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
