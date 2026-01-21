//! Expression compilation for WASM backend.

use super::error::WasmError;
use super::types::syn_type_to_wasm;
use std::collections::HashMap;
use walrus::ir::{BinaryOp, UnaryOp};
use walrus::{FunctionId, InstrSeqBuilder, LocalId, ValType};

/// Context for compiling expressions within a function.
pub struct ExprContext<'a, 'b> {
    /// Map from variable names to local IDs
    pub locals: &'a HashMap<String, LocalId>,
    /// Map from function names to function IDs
    pub functions: &'a HashMap<String, FunctionId>,
    /// The current instruction sequence builder
    pub builder: &'b mut InstrSeqBuilder<'a>,
}

impl<'a, 'b> ExprContext<'a, 'b> {
    /// Compile an expression and push its result onto the stack.
    pub fn compile_expr(&mut self, expr: &syn::Expr) -> Result<(), WasmError> {
        match expr {
            syn::Expr::Lit(lit) => self.compile_lit(&lit.lit),
            syn::Expr::Binary(binary) => self.compile_binary(binary),
            syn::Expr::Unary(unary) => self.compile_unary(unary),
            syn::Expr::Path(path) => self.compile_path(path),
            syn::Expr::Call(call) => self.compile_call(call),
            syn::Expr::Paren(paren) => self.compile_expr(&paren.expr),
            syn::Expr::Block(block) => self.compile_block_expr(&block.block),
            syn::Expr::If(if_expr) => self.compile_if(if_expr),
            syn::Expr::Cast(cast) => self.compile_cast(cast),
            syn::Expr::Assign(assign) => self.compile_assign(assign),
            syn::Expr::Return(ret) => self.compile_return(ret),
            other => Err(WasmError::Unsupported(format!(
                "Expression type: {}",
                quote::quote!(#other)
            ))),
        }
    }

    fn compile_lit(&mut self, lit: &syn::Lit) -> Result<(), WasmError> {
        match lit {
            syn::Lit::Int(int) => {
                let value: i64 = int
                    .base10_parse()
                    .map_err(|e| WasmError::TypeError(e.to_string()))?;
                // Determine type from suffix or default to i32
                let suffix = int.suffix();
                if suffix == "i64" || suffix == "u64" {
                    self.builder.i64_const(value);
                } else {
                    self.builder.i32_const(value as i32);
                }
                Ok(())
            }
            syn::Lit::Float(float) => {
                let suffix = float.suffix();
                if suffix == "f32" {
                    let value: f32 = float
                        .base10_parse()
                        .map_err(|e| WasmError::TypeError(e.to_string()))?;
                    self.builder.f32_const(value);
                } else {
                    let value: f64 = float
                        .base10_parse()
                        .map_err(|e| WasmError::TypeError(e.to_string()))?;
                    self.builder.f64_const(value);
                }
                Ok(())
            }
            syn::Lit::Bool(b) => {
                self.builder.i32_const(if b.value { 1 } else { 0 });
                Ok(())
            }
            _ => Err(WasmError::Unsupported("Unsupported literal type".to_string())),
        }
    }

    fn compile_binary(&mut self, binary: &syn::ExprBinary) -> Result<(), WasmError> {
        // Compile left and right operands
        self.compile_expr(&binary.left)?;
        self.compile_expr(&binary.right)?;

        // Apply the binary operation
        // For now, assume i32 operations (we'd need type inference for proper handling)
        let op = match &binary.op {
            syn::BinOp::Add(_) => BinaryOp::I32Add,
            syn::BinOp::Sub(_) => BinaryOp::I32Sub,
            syn::BinOp::Mul(_) => BinaryOp::I32Mul,
            syn::BinOp::Div(_) => BinaryOp::I32DivS,
            syn::BinOp::Rem(_) => BinaryOp::I32RemS,
            syn::BinOp::And(_) => BinaryOp::I32And,
            syn::BinOp::Or(_) => BinaryOp::I32Or,
            syn::BinOp::BitXor(_) => BinaryOp::I32Xor,
            syn::BinOp::BitAnd(_) => BinaryOp::I32And,
            syn::BinOp::BitOr(_) => BinaryOp::I32Or,
            syn::BinOp::Shl(_) => BinaryOp::I32Shl,
            syn::BinOp::Shr(_) => BinaryOp::I32ShrS,
            syn::BinOp::Eq(_) => BinaryOp::I32Eq,
            syn::BinOp::Ne(_) => BinaryOp::I32Ne,
            syn::BinOp::Lt(_) => BinaryOp::I32LtS,
            syn::BinOp::Le(_) => BinaryOp::I32LeS,
            syn::BinOp::Gt(_) => BinaryOp::I32GtS,
            syn::BinOp::Ge(_) => BinaryOp::I32GeS,
            other => {
                return Err(WasmError::Unsupported(format!(
                    "Binary operator: {}",
                    quote::quote!(#other)
                )))
            }
        };

        self.builder.binop(op);
        Ok(())
    }

    fn compile_unary(&mut self, unary: &syn::ExprUnary) -> Result<(), WasmError> {
        self.compile_expr(&unary.expr)?;

        match &unary.op {
            syn::UnOp::Neg(_) => {
                // Negate: 0 - value
                // Push 0, swap, subtract
                self.builder.i32_const(0);
                // We need to swap - unfortunately WASM doesn't have swap
                // We'll use a different approach: push 0 first, then value, then sub
                // Actually we already pushed value, so let's use: value -> -value
                // -x = 0 - x, but we have x on stack
                // We need to compute 0 - x
                // Unfortunately this requires reordering. Let's just multiply by -1
                self.builder.i32_const(-1);
                self.builder.binop(BinaryOp::I32Mul);
            }
            syn::UnOp::Not(_) => {
                // Logical not: value == 0
                self.builder.unop(UnaryOp::I32Eqz);
            }
            syn::UnOp::Deref(_) => {
                // Dereference not supported in no_std WASM
                return Err(WasmError::Unsupported("Dereference operator".to_string()));
            }
            _ => {
                return Err(WasmError::Unsupported("Unknown unary operator".to_string()));
            }
        }
        Ok(())
    }

    fn compile_path(&mut self, path: &syn::ExprPath) -> Result<(), WasmError> {
        if path.path.segments.len() == 1 {
            let name = path.path.segments[0].ident.to_string();
            if let Some(&local_id) = self.locals.get(&name) {
                self.builder.local_get(local_id);
                return Ok(());
            }
            // Could be a constant or function reference
            return Err(WasmError::UnknownIdentifier(name));
        }
        Err(WasmError::Unsupported(format!(
            "Path expression: {}",
            quote::quote!(#path)
        )))
    }

    fn compile_call(&mut self, call: &syn::ExprCall) -> Result<(), WasmError> {
        // Get function name
        let func_name = match call.func.as_ref() {
            syn::Expr::Path(path) if path.path.segments.len() == 1 => {
                path.path.segments[0].ident.to_string()
            }
            _ => {
                return Err(WasmError::Unsupported(format!(
                    "Call expression: {}",
                    quote::quote!(#call)
                )))
            }
        };

        // Compile arguments (push onto stack)
        for arg in &call.args {
            self.compile_expr(arg)?;
        }

        // Look up function and call it
        if let Some(&func_id) = self.functions.get(&func_name) {
            self.builder.call(func_id);
            Ok(())
        } else {
            Err(WasmError::UnknownIdentifier(func_name))
        }
    }

    fn compile_block_expr(&mut self, block: &syn::Block) -> Result<(), WasmError> {
        for stmt in &block.stmts {
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    fn compile_if(&mut self, if_expr: &syn::ExprIf) -> Result<(), WasmError> {
        // Compile condition
        self.compile_expr(&if_expr.cond)?;

        // For now, we'll use a simple if without proper block handling
        // This is a simplification - proper implementation would need
        // dangling_instr_seq for the then/else blocks
        
        // Compile then block inline for simple cases
        // A proper implementation would create separate instruction sequences
        
        Err(WasmError::Unsupported(
            "If expressions require more complex handling - use statements instead".to_string(),
        ))
    }

    fn compile_cast(&mut self, cast: &syn::ExprCast) -> Result<(), WasmError> {
        self.compile_expr(&cast.expr)?;

        // Get target type
        let target = syn_type_to_wasm(&cast.ty)?;
        
        // For now, assume source is i32 (we'd need type inference for proper handling)
        match target {
            ValType::I32 => { /* Already i32, no conversion needed */ }
            ValType::I64 => {
                self.builder.unop(UnaryOp::I64ExtendSI32);
            }
            ValType::F32 => {
                self.builder.unop(UnaryOp::F32ConvertSI32);
            }
            ValType::F64 => {
                self.builder.unop(UnaryOp::F64ConvertSI32);
            }
            _ => {
                return Err(WasmError::TypeError(format!(
                    "Cannot cast to {target:?}"
                )))
            }
        }
        Ok(())
    }

    fn compile_assign(&mut self, assign: &syn::ExprAssign) -> Result<(), WasmError> {
        // Compile the value
        self.compile_expr(&assign.right)?;

        // Get the target local
        if let syn::Expr::Path(path) = assign.left.as_ref() {
            if path.path.segments.len() == 1 {
                let name = path.path.segments[0].ident.to_string();
                if let Some(&local_id) = self.locals.get(&name) {
                    self.builder.local_set(local_id);
                    return Ok(());
                }
                return Err(WasmError::UnknownIdentifier(name));
            }
        }
        Err(WasmError::Unsupported(format!(
            "Assignment target: {}",
            quote::quote!(#assign.left)
        )))
    }

    fn compile_return(&mut self, ret: &syn::ExprReturn) -> Result<(), WasmError> {
        if let Some(expr) = &ret.expr {
            self.compile_expr(expr)?;
        }
        self.builder.return_();
        Ok(())
    }

    /// Compile a statement.
    pub fn compile_stmt(&mut self, stmt: &syn::Stmt) -> Result<(), WasmError> {
        match stmt {
            syn::Stmt::Local(_local) => {
                // Local variable declarations are handled at function level
                // The initialization expression would be compiled here
                // For now, skip (variables are pre-declared)
                Ok(())
            }
            syn::Stmt::Expr(expr, _semi) => {
                self.compile_expr(expr)?;
                // If there's a semicolon, drop the result
                // self.builder.drop();
                Ok(())
            }
            syn::Stmt::Item(_) => {
                // Nested items not supported
                Err(WasmError::Unsupported("Nested items".to_string()))
            }
            syn::Stmt::Macro(_) => Err(WasmError::Unsupported("Macros".to_string())),
        }
    }
}
