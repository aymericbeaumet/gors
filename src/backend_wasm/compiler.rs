//! Main WASM compiler implementation.

use super::error::WasmError;
use super::expr::{ExprContext, StringLiterals};
use super::types::{params_to_wasm, return_type_to_wasm, syn_type_to_wasm};
use std::collections::HashMap;
use walrus::{ConstExpr, DataKind, FunctionBuilder, FunctionId, LocalId, MemoryId, Module, ValType, ir::Value};

/// WebAssembly compiler that translates syn::File to WASM bytecode.
pub struct WasmCompiler {
    module: Module,
    /// Map from function names to their IDs
    functions: HashMap<String, FunctionId>,
    /// Memory for storing string literals
    memory: MemoryId,
}

impl WasmCompiler {
    /// Create a new WASM compiler.
    pub fn new() -> Self {
        let mut module = Module::default();

        // Add linear memory (1 page = 64KB, enough for string literals)
        // add_local(shared, memory64, initial_pages, max_pages, page_size_log2)
        let memory = module.memories.add_local(false, false, 1, None, None);
        module.exports.add("memory", memory);

        // Add print_i32 import for integer output
        // Import from "env" module, function "print_i32"
        let print_i32_type = module.types.add(&[ValType::I32], &[]);
        let (print_i32, _) = module.add_import_func("env", "print_i32", print_i32_type);

        // Add print_str import for string output (takes pointer and length)
        let print_str_type = module.types.add(&[ValType::I32, ValType::I32], &[]);
        let (print_str, _) = module.add_import_func("env", "print_str", print_str_type);

        let mut functions = HashMap::new();
        functions.insert("print_i32".to_string(), print_i32);
        functions.insert("print_str".to_string(), print_str);

        Self {
            module,
            functions,
            memory,
        }
    }

    /// Compile a syn::File to WASM.
    pub fn compile(&mut self, file: &syn::File) -> Result<(), WasmError> {
        // First pass: collect all function signatures
        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                self.declare_function(func)?;
            }
        }

        // Second pass: compile function bodies
        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                self.compile_function(func)?;
            }
        }

        Ok(())
    }

    /// Declare a function (add its signature without body).
    fn declare_function(&mut self, func: &syn::ItemFn) -> Result<(), WasmError> {
        let name = func.sig.ident.to_string();

        // Skip if already declared (e.g., imports)
        if self.functions.contains_key(&name) {
            return Ok(());
        }

        let params = params_to_wasm(&func.sig.inputs)?;
        let results: Vec<ValType> = return_type_to_wasm(&func.sig.output)?
            .into_iter()
            .collect();

        // Create function type
        let _type_id = self.module.types.add(&params, &results);

        // Create a placeholder function (will be replaced when compiling body)
        let mut builder = FunctionBuilder::new(&mut self.module.types, &params, &results);
        
        // Add empty body for now
        if results.is_empty() {
            // No return value needed
        } else {
            // Push a default return value
            match results[0] {
                ValType::I32 => { builder.func_body().i32_const(0); }
                ValType::I64 => { builder.func_body().i64_const(0); }
                ValType::F32 => { builder.func_body().f32_const(0.0); }
                ValType::F64 => { builder.func_body().f64_const(0.0); }
                _ => {}
            }
        }

        let func_id = builder.finish(vec![], &mut self.module.funcs);
        self.functions.insert(name, func_id);

        // Note: Export is added in compile_function after body is compiled

        Ok(())
    }

    /// Compile a function body.
    fn compile_function(&mut self, func: &syn::ItemFn) -> Result<(), WasmError> {
        let name = func.sig.ident.to_string();
        
        // Get function info
        let params = params_to_wasm(&func.sig.inputs)?;
        let results: Vec<ValType> = return_type_to_wasm(&func.sig.output)?
            .into_iter()
            .collect();

        // Create a new function builder
        let mut builder = FunctionBuilder::new(&mut self.module.types, &params, &results);

        // Create locals map
        let mut locals: HashMap<String, LocalId> = HashMap::new();

        // Add parameters as locals
        let mut param_locals = Vec::new();
        for (i, arg) in func.sig.inputs.iter().enumerate() {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                    let param_name = pat_ident.ident.to_string();
                    let local_id = self.module.locals.add(params[i]);
                    locals.insert(param_name, local_id);
                    param_locals.push(local_id);
                }
            }
        }

        // Collect local variables from the function body
        self.collect_locals(&func.block, &mut locals)?;

        // String literals collector
        let mut string_literals = StringLiterals::new();

        // Compile the function body
        {
            let mut body = builder.func_body();
            let mut ctx = ExprContext {
                locals: &locals,
                functions: &self.functions,
                builder: &mut body,
                string_literals: &mut string_literals,
            };

            // Compile each statement
            for stmt in &func.block.stmts {
                self.compile_stmt(&mut ctx, stmt)?;
            }
        }

        // Finish the function
        let new_func_id = builder.finish(param_locals, &mut self.module.funcs);

        // Add collected string literals to memory
        for (offset, _len, content) in string_literals.into_vec() {
            self.module.data.add(
                DataKind::Active {
                    memory: self.memory,
                    offset: ConstExpr::Value(Value::I32(offset as i32)),
                },
                content.into_bytes(),
            );
        }

        // Update the function map
        self.functions.insert(name.clone(), new_func_id);

        // Re-export main if this is main
        if name == "main" {
            // Remove old export if exists and add new one
            self.module.exports.add(&name, new_func_id);
        }

        Ok(())
    }

    /// Collect local variable declarations from a block.
    fn collect_locals(
        &mut self,
        block: &syn::Block,
        locals: &mut HashMap<String, LocalId>,
    ) -> Result<(), WasmError> {
        for stmt in &block.stmts {
            if let syn::Stmt::Local(local) = stmt {
                if let syn::Pat::Ident(pat_ident) = &local.pat {
                    let name = pat_ident.ident.to_string();
                    
                    // Determine type from annotation or initializer
                    let val_type = if let Some(ty) = &pat_ident.by_ref {
                        // Reference type
                        let _ = ty;
                        ValType::I32
                    } else if let Some(local_init) = &local.init {
                        // Try to infer from initializer (simplified)
                        self.infer_expr_type(&local_init.expr)?
                    } else {
                        // Default to i32
                        ValType::I32
                    };

                    if let std::collections::hash_map::Entry::Vacant(e) = locals.entry(name) {
                        let local_id = self.module.locals.add(val_type);
                        e.insert(local_id);
                    }
                } else if let syn::Pat::Type(pat_type) = &local.pat {
                    if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                        let name = pat_ident.ident.to_string();
                        let val_type = syn_type_to_wasm(&pat_type.ty)?;
                        
                        if let std::collections::hash_map::Entry::Vacant(e) = locals.entry(name) {
                            let local_id = self.module.locals.add(val_type);
                            e.insert(local_id);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Infer the WASM type of an expression (simplified).
    fn infer_expr_type(&self, expr: &syn::Expr) -> Result<ValType, WasmError> {
        match expr {
            syn::Expr::Lit(lit) => match &lit.lit {
                syn::Lit::Int(int) => {
                    let suffix = int.suffix();
                    if suffix == "i64" || suffix == "u64" {
                        Ok(ValType::I64)
                    } else {
                        Ok(ValType::I32)
                    }
                }
                syn::Lit::Float(float) => {
                    let suffix = float.suffix();
                    if suffix == "f32" {
                        Ok(ValType::F32)
                    } else {
                        Ok(ValType::F64)
                    }
                }
                syn::Lit::Bool(_) => Ok(ValType::I32),
                _ => Ok(ValType::I32),
            },
            syn::Expr::Binary(_) => Ok(ValType::I32), // Simplified
            syn::Expr::Paren(paren) => self.infer_expr_type(&paren.expr),
            syn::Expr::Cast(cast) => syn_type_to_wasm(&cast.ty),
            _ => Ok(ValType::I32), // Default to i32
        }
    }

    /// Compile a statement.
    fn compile_stmt<'a, 'b, 'c>(&self, ctx: &mut ExprContext<'a, 'b, 'c>, stmt: &syn::Stmt) -> Result<(), WasmError> {
        match stmt {
            syn::Stmt::Local(local) => {
                // Handle local variable initialization
                if let Some(init) = &local.init {
                    ctx.compile_expr(&init.expr)?;
                    
                    // Get the local ID
                    let name = if let syn::Pat::Ident(pat_ident) = &local.pat {
                        pat_ident.ident.to_string()
                    } else if let syn::Pat::Type(pat_type) = &local.pat {
                        if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                            pat_ident.ident.to_string()
                        } else {
                            return Err(WasmError::Unsupported("Complex pattern".to_string()));
                        }
                    } else {
                        return Err(WasmError::Unsupported("Complex pattern".to_string()));
                    };

                    if let Some(&local_id) = ctx.locals.get(&name) {
                        ctx.builder.local_set(local_id);
                    }
                }
                Ok(())
            }
            syn::Stmt::Expr(expr, semi) => {
                ctx.compile_expr(expr)?;
                // If there's a semicolon and the expression produces a value, drop it
                if semi.is_some() {
                    // We'd need type info to know if we should drop
                    // For now, assume expressions like assignments don't produce values
                }
                Ok(())
            }
            syn::Stmt::Item(_) => Err(WasmError::Unsupported("Nested items".to_string())),
            syn::Stmt::Macro(stmt_macro) => {
                // Convert macro statement to expression and compile
                let mac_expr = syn::ExprMacro {
                    attrs: stmt_macro.attrs.clone(),
                    mac: stmt_macro.mac.clone(),
                };
                ctx.compile_expr(&syn::Expr::Macro(mac_expr))
            }
        }
    }

    /// Emit the compiled WASM module as bytes.
    ///
    /// This runs optimization passes (garbage collection to remove unused items)
    /// before emitting the final binary.
    pub fn emit(mut self) -> Vec<u8> {
        // Run garbage collection pass to remove unused functions, globals, etc.
        walrus::passes::gc::run(&mut self.module);

        self.module.emit_wasm()
    }
}

impl Default for WasmCompiler {
    fn default() -> Self {
        Self::new()
    }
}
