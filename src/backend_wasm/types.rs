//! Type translation from Rust/syn types to WASM types.

use super::WasmError;
use walrus::ValType;

/// Convert a syn::Type to a WASM ValType.
pub fn syn_type_to_wasm(ty: &syn::Type) -> Result<ValType, WasmError> {
    match ty {
        syn::Type::Path(type_path) => {
            let path = &type_path.path;
            if path.segments.len() == 1 {
                let ident = &path.segments[0].ident;
                match ident.to_string().as_str() {
                    "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "isize" | "usize" | "bool" => {
                        Ok(ValType::I32)
                    }
                    "i64" | "u64" => Ok(ValType::I64),
                    "f32" => Ok(ValType::F32),
                    "f64" => Ok(ValType::F64),
                    other => Err(WasmError::TypeError(format!("Unsupported type: {other}"))),
                }
            } else {
                Err(WasmError::TypeError(format!(
                    "Unsupported path type: {}",
                    quote::quote!(#type_path)
                )))
            }
        }
        syn::Type::Tuple(tuple) if tuple.elems.is_empty() => {
            // Unit type () - no return value
            Err(WasmError::TypeError(
                "Unit type has no WASM representation".to_string(),
            ))
        }
        syn::Type::Reference(reference) => {
            // References become i32 pointers in WASM
            let _ = reference; // We don't validate the inner type for now
            Ok(ValType::I32)
        }
        other => Err(WasmError::TypeError(format!(
            "Unsupported type: {}",
            quote::quote!(#other)
        ))),
    }
}

/// Check if a type is the unit type ().
pub fn is_unit_type(ty: &syn::Type) -> bool {
    matches!(ty, syn::Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// Convert a syn::ReturnType to an optional WASM ValType.
pub fn return_type_to_wasm(ret: &syn::ReturnType) -> Result<Option<ValType>, WasmError> {
    match ret {
        syn::ReturnType::Default => Ok(None),
        syn::ReturnType::Type(_, ty) => {
            if is_unit_type(ty) {
                Ok(None)
            } else {
                syn_type_to_wasm(ty).map(Some)
            }
        }
    }
}

/// Get the WASM types for function parameters.
pub fn params_to_wasm(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::token::Comma>,
) -> Result<Vec<ValType>, WasmError> {
    inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => syn_type_to_wasm(&pat_type.ty),
            syn::FnArg::Receiver(_) => Err(WasmError::Unsupported(
                "self receivers not supported".to_string(),
            )),
        })
        .collect()
}
