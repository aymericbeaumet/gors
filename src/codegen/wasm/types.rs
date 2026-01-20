//! WASM type system and type inference.

use std::collections::HashMap;

/// WASM value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmType {
    /// 32-bit integer (i8, i16, i32, isize, u8, u16, u32, usize, bool)
    I32,
    /// 64-bit integer (i64, u64)
    I64,
    /// 32-bit float (f32)
    F32,
    /// 64-bit float (f64)
    F64,
    /// Pointer type (String, slices) - represented as i32 in WASM
    Ptr,
    /// Void/unit type (no return value)
    Void,
}

impl WasmType {
    /// Get the WAT type name.
    pub fn wat_name(&self) -> &'static str {
        match self {
            Self::I32 | Self::Ptr => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::Void => "",
        }
    }

    /// Check if this type is numeric (can be used in arithmetic).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::I32 | Self::I64 | Self::F32 | Self::F64)
    }

    /// Check if this type is an integer type.
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::I32 | Self::I64)
    }

    /// Check if this type is a float type.
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// Parse a Rust type name into a WASM type.
    pub fn from_rust_type(name: &str) -> Self {
        match name {
            "bool" => Self::I32,
            "i8" | "i16" | "i32" | "isize" => Self::I32,
            "u8" | "u16" | "u32" | "usize" => Self::I32,
            "i64" => Self::I64,
            "u64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "String" | "&str" => Self::Ptr,
            "()" => Self::Void,
            _ => Self::I32, // Default to i32 for unknown types
        }
    }
}

impl std::fmt::Display for WasmType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.wat_name())
    }
}

/// Function signature for WASM.
#[derive(Debug, Clone)]
pub struct FunctionSig {
    /// Parameter types
    pub params: Vec<WasmType>,
    /// Return type (Void if no return)
    pub result: WasmType,
}

/// Context for type tracking during compilation.
#[derive(Debug, Default)]
pub struct TypeContext {
    /// Local variable types (name -> type)
    locals: HashMap<String, WasmType>,
    /// Function signatures (name -> signature)
    functions: HashMap<String, FunctionSig>,
    /// Local variable indices (name -> index)
    local_indices: HashMap<String, u32>,
    /// Next available local index
    next_local: u32,
}

impl TypeContext {
    /// Create a new type context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all local variables (for new function scope).
    pub fn clear_locals(&mut self) {
        self.locals.clear();
        self.local_indices.clear();
        self.next_local = 0;
    }

    /// Define a local variable with a type.
    pub fn define_local(&mut self, name: &str, ty: WasmType) -> u32 {
        let index = self.next_local;
        self.locals.insert(name.to_string(), ty);
        self.local_indices.insert(name.to_string(), index);
        self.next_local += 1;
        index
    }

    /// Get the type of a local variable.
    pub fn get_local_type(&self, name: &str) -> Option<WasmType> {
        self.locals.get(name).copied()
    }

    /// Get the index of a local variable.
    pub fn get_local_index(&self, name: &str) -> Option<u32> {
        self.local_indices.get(name).copied()
    }

    /// Define a function signature.
    pub fn define_function(&mut self, name: &str, sig: FunctionSig) {
        self.functions.insert(name.to_string(), sig);
    }

    /// Get a function signature.
    pub fn get_function(&self, name: &str) -> Option<&FunctionSig> {
        self.functions.get(name)
    }

    /// Get all local variables with their types (for local declarations).
    pub fn get_locals(&self) -> Vec<(String, WasmType)> {
        let mut locals: Vec<_> = self.local_indices
            .iter()
            .map(|(name, &idx)| (idx, name.clone(), self.locals[name]))
            .collect();
        locals.sort_by_key(|(idx, _, _)| *idx);
        locals.into_iter().map(|(_, name, ty)| (name, ty)).collect()
    }

    /// Get the number of locals.
    pub fn local_count(&self) -> u32 {
        self.next_local
    }
}

/// Infer the type of a syn expression.
pub fn infer_expr_type(expr: &syn::Expr, ctx: &TypeContext) -> WasmType {
    match expr {
        syn::Expr::Lit(lit) => infer_lit_type(&lit.lit),
        syn::Expr::Path(path) => {
            // Look up variable type
            if let Some(ident) = path.path.get_ident() {
                ctx.get_local_type(&ident.to_string()).unwrap_or(WasmType::I32)
            } else {
                WasmType::I32
            }
        }
        syn::Expr::Binary(binary) => {
            // Compound assignment operators don't leave a value on the stack
            match binary.op {
                syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_) => WasmType::Void,
                // For comparison operators, result is bool (i32)
                syn::BinOp::Eq(_)
                | syn::BinOp::Ne(_)
                | syn::BinOp::Lt(_)
                | syn::BinOp::Le(_)
                | syn::BinOp::Gt(_)
                | syn::BinOp::Ge(_)
                | syn::BinOp::And(_)
                | syn::BinOp::Or(_) => WasmType::I32,
                _ => {
                    // For arithmetic, use the type of the left operand
                    infer_expr_type(&binary.left, ctx)
                }
            }
        }
        syn::Expr::Unary(unary) => infer_expr_type(&unary.expr, ctx),
        syn::Expr::Paren(paren) => infer_expr_type(&paren.expr, ctx),
        syn::Expr::Call(_) => WasmType::I32, // Default for function calls
        syn::Expr::Macro(mac) => {
            // Check if it's a println!/print! macro (returns void)
            let macro_name = mac.mac.path.segments.iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            match macro_name.as_str() {
                "println" | "print" => WasmType::Void,
                _ => WasmType::I32,
            }
        }
        syn::Expr::Cast(cast) => {
            // Get the target type from the cast
            if let syn::Type::Path(type_path) = cast.ty.as_ref() {
                if let Some(ident) = type_path.path.get_ident() {
                    return WasmType::from_rust_type(&ident.to_string());
                }
            }
            WasmType::I32
        }
        // Assignment expressions don't leave a value on the stack
        syn::Expr::Assign(_) => WasmType::Void,
        // Control flow that doesn't return a value
        syn::Expr::If(_) | syn::Expr::While(_) | syn::Expr::Loop(_) | syn::Expr::Block(_) => {
            WasmType::Void
        }
        syn::Expr::Return(_) | syn::Expr::Break(_) | syn::Expr::Continue(_) => WasmType::Void,
        _ => WasmType::I32 // Default to i32
    }
}

/// Infer the type of a literal.
pub fn infer_lit_type(lit: &syn::Lit) -> WasmType {
    match lit {
        syn::Lit::Int(int_lit) => {
            // Check the suffix if present
            let suffix = int_lit.suffix();
            if suffix.is_empty() {
                WasmType::I32
            } else {
                WasmType::from_rust_type(suffix)
            }
        }
        syn::Lit::Float(float_lit) => {
            let suffix = float_lit.suffix();
            if suffix.is_empty() || suffix == "f64" {
                WasmType::F64
            } else {
                WasmType::F32
            }
        }
        syn::Lit::Bool(_) => WasmType::I32,
        syn::Lit::Str(_) => WasmType::Ptr,
        syn::Lit::Char(_) => WasmType::I32,
        _ => WasmType::I32,
    }
}
