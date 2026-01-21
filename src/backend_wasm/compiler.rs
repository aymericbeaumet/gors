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
        // Collect all function items
        let func_items: Vec<_> = file.items.iter()
            .filter_map(|item| {
                if let syn::Item::Fn(func) = item {
                    Some(func)
                } else {
                    None
                }
            })
            .collect();

        // Pass 1: Create placeholder functions to get IDs for ALL functions
        // This ensures that when compiling any function body, all function IDs are known
        let mut placeholder_ids: HashMap<String, FunctionId> = HashMap::new();
        
        for func in &func_items {
            let name = func.sig.ident.to_string();
            
            if self.functions.contains_key(&name) {
                continue;
            }

            let params = params_to_wasm(&func.sig.inputs)?;
            let results: Vec<ValType> = return_type_to_wasm(&func.sig.output)?
                .into_iter()
                .collect();

            // Create placeholder function
            let mut builder = FunctionBuilder::new(&mut self.module.types, &params, &results);
            
            if !results.is_empty() {
                match results[0] {
                    ValType::I32 => { builder.func_body().i32_const(0); }
                    ValType::I64 => { builder.func_body().i64_const(0); }
                    ValType::F32 => { builder.func_body().f32_const(0.0); }
                    ValType::F64 => { builder.func_body().f64_const(0.0); }
                    _ => {}
                }
            }

            let func_id = builder.finish(vec![], &mut self.module.funcs);
            placeholder_ids.insert(name.clone(), func_id);
            self.functions.insert(name, func_id);
        }

        // Pass 2: Compile real function bodies
        // The self.functions map now has ALL function IDs (placeholders)
        let mut new_ids: HashMap<String, FunctionId> = HashMap::new();
        
        for func in &func_items {
            let new_id = self.compile_function_body(func)?;
            let name = func.sig.ident.to_string();
            new_ids.insert(name, new_id);
        }

        // Pass 3: Update the function map and patch call instructions
        // Build a mapping from placeholder ID to real ID
        let id_mapping: HashMap<FunctionId, FunctionId> = placeholder_ids.iter()
            .filter_map(|(name, &placeholder_id)| {
                new_ids.get(name).map(|&new_id| (placeholder_id, new_id))
            })
            .collect();

        // Patch all call instructions in all new functions
        for &new_id in new_ids.values() {
            self.patch_function_calls(new_id, &id_mapping);
        }

        // Update the function map with the real IDs
        for (name, new_id) in new_ids {
            self.functions.insert(name, new_id);
        }

        // Delete placeholder functions (they're no longer needed after patching)
        for &placeholder_id in placeholder_ids.values() {
            self.module.funcs.delete(placeholder_id);
        }

        Ok(())
    }

    /// Patch all call instructions in a function to use the new function IDs.
    fn patch_function_calls(&mut self, func_id: FunctionId, id_mapping: &HashMap<FunctionId, FunctionId>) {
        use walrus::ir::*;
        
        let func = self.module.funcs.get_mut(func_id);
        if let walrus::FunctionKind::Local(local_func) = &mut func.kind {
            // Visit all instructions and patch Call instructions
            let entry = local_func.entry_block();
            let mut to_visit = vec![entry];
            let mut visited = std::collections::HashSet::new();
            
            while let Some(block_id) = to_visit.pop() {
                if !visited.insert(block_id) {
                    continue;
                }
                
                let block = local_func.block_mut(block_id);
                for (instr, _) in block.instrs.iter_mut() {
                    match instr {
                        Instr::Call(call) => {
                            if let Some(&new_id) = id_mapping.get(&call.func) {
                                call.func = new_id;
                            }
                        }
                        Instr::Block(b) => to_visit.push(b.seq),
                        Instr::Loop(l) => to_visit.push(l.seq),
                        Instr::IfElse(ie) => {
                            to_visit.push(ie.consequent);
                            to_visit.push(ie.alternative);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Compile a function body and return the new function ID.
    fn compile_function_body(&mut self, func: &syn::ItemFn) -> Result<FunctionId, WasmError> {
        let name = func.sig.ident.to_string();
        
        let params = params_to_wasm(&func.sig.inputs)?;
        let results: Vec<ValType> = return_type_to_wasm(&func.sig.output)?
            .into_iter()
            .collect();

        let mut builder = FunctionBuilder::new(&mut self.module.types, &params, &results);

        let mut locals: HashMap<String, LocalId> = HashMap::new();
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

        self.collect_locals(&func.block, &mut locals)?;

        let mut string_literals = StringLiterals::new();

        {
            let mut body = builder.func_body();
            let mut ctx = ExprContext {
                locals: &locals,
                functions: &self.functions,  // Uses placeholder IDs
                builder: &mut body,
                string_literals: &mut string_literals,
            };

            for stmt in &func.block.stmts {
                self.compile_stmt(&mut ctx, stmt)?;
            }
        }

        let func_id = builder.finish(param_locals, &mut self.module.funcs);

        for (offset, _len, content) in string_literals.into_vec() {
            self.module.data.add(
                DataKind::Active {
                    memory: self.memory,
                    offset: ConstExpr::Value(Value::I32(offset as i32)),
                },
                content.into_bytes(),
            );
        }

        // Export main
        if name == "main" {
            self.module.exports.add(&name, func_id);
        }

        Ok(func_id)
    }

    /// Collect local variable declarations from a block, recursively.
    fn collect_locals(
        &mut self,
        block: &syn::Block,
        locals: &mut HashMap<String, LocalId>,
    ) -> Result<(), WasmError> {
        for stmt in &block.stmts {
            match stmt {
                syn::Stmt::Local(local) => {
                    self.collect_pat_locals(&local.pat, local.init.as_ref().map(|i| &i.expr), locals)?;
                }
                syn::Stmt::Expr(expr, _) => {
                    // Recurse into nested blocks, while loops, etc.
                    self.collect_expr_locals(expr, locals)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Collect locals from expressions that contain blocks (while, if, blocks, etc.)
    fn collect_expr_locals(
        &mut self,
        expr: &syn::Expr,
        locals: &mut HashMap<String, LocalId>,
    ) -> Result<(), WasmError> {
        match expr {
            syn::Expr::Block(block) => {
                self.collect_locals(&block.block, locals)?;
            }
            syn::Expr::While(while_expr) => {
                self.collect_locals(&while_expr.body, locals)?;
            }
            syn::Expr::Loop(loop_expr) => {
                self.collect_locals(&loop_expr.body, locals)?;
            }
            syn::Expr::If(if_expr) => {
                self.collect_locals(&if_expr.then_branch, locals)?;
                if let Some((_, else_branch)) = &if_expr.else_branch {
                    self.collect_expr_locals(else_branch, locals)?;
                }
            }
            syn::Expr::ForLoop(for_loop) => {
                self.collect_locals(&for_loop.body, locals)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Collect locals from a pattern, recursively handling tuples.
    fn collect_pat_locals(
        &mut self,
        pat: &syn::Pat,
        init: Option<&Box<syn::Expr>>,
        locals: &mut HashMap<String, LocalId>,
    ) -> Result<(), WasmError> {
        match pat {
            syn::Pat::Ident(pat_ident) => {
                let name = pat_ident.ident.to_string();
                
                // Determine type from initializer
                let val_type = if let Some(init_expr) = init {
                    self.infer_expr_type(init_expr)?
                } else {
                    ValType::I32
                };

                if let std::collections::hash_map::Entry::Vacant(e) = locals.entry(name) {
                    let local_id = self.module.locals.add(val_type);
                    e.insert(local_id);
                }
            }
            syn::Pat::Type(pat_type) => {
                if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                    let name = pat_ident.ident.to_string();
                    let val_type = syn_type_to_wasm(&pat_type.ty)?;
                    
                    if let std::collections::hash_map::Entry::Vacant(e) = locals.entry(name) {
                        let local_id = self.module.locals.add(val_type);
                        e.insert(local_id);
                    }
                }
            }
            syn::Pat::Tuple(pat_tuple) => {
                // Handle tuple pattern - extract corresponding init expressions if available
                let tuple_elems = if let Some(init_expr) = init {
                    if let syn::Expr::Tuple(init_tuple) = init_expr.as_ref() {
                        Some(&init_tuple.elems)
                    } else {
                        None
                    }
                } else {
                    None
                };

                for (i, elem_pat) in pat_tuple.elems.iter().enumerate() {
                    let elem_init = tuple_elems.and_then(|elems| elems.get(i).map(|e| {
                        // Box the expression reference
                        Box::new(e.clone())
                    }));
                    self.collect_pat_locals(elem_pat, elem_init.as_ref(), locals)?;
                }
            }
            _ => {}
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
                    self.compile_pat_assignment(ctx, &local.pat, &init.expr)?;
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

    /// Compile a pattern assignment, handling tuples.
    fn compile_pat_assignment<'a, 'b, 'c>(
        &self,
        ctx: &mut ExprContext<'a, 'b, 'c>,
        pat: &syn::Pat,
        expr: &syn::Expr,
    ) -> Result<(), WasmError> {
        match pat {
            syn::Pat::Ident(pat_ident) => {
                ctx.compile_expr(expr)?;
                let name = pat_ident.ident.to_string();
                if let Some(&local_id) = ctx.locals.get(&name) {
                    ctx.builder.local_set(local_id);
                }
                Ok(())
            }
            syn::Pat::Type(pat_type) => {
                if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                    ctx.compile_expr(expr)?;
                    let name = pat_ident.ident.to_string();
                    if let Some(&local_id) = ctx.locals.get(&name) {
                        ctx.builder.local_set(local_id);
                    }
                }
                Ok(())
            }
            syn::Pat::Tuple(pat_tuple) => {
                // For tuple patterns, we need to match with tuple expressions
                if let syn::Expr::Tuple(expr_tuple) = expr {
                    // Compile and assign each element
                    for (elem_pat, elem_expr) in pat_tuple.elems.iter().zip(expr_tuple.elems.iter()) {
                        self.compile_pat_assignment(ctx, elem_pat, elem_expr)?;
                    }
                    Ok(())
                } else {
                    Err(WasmError::Unsupported(
                        "Tuple pattern requires tuple expression".to_string(),
                    ))
                }
            }
            _ => Err(WasmError::Unsupported(format!(
                "Pattern type: {}",
                quote::quote!(#pat)
            ))),
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
