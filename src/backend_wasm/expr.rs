//! Expression compilation for WASM backend.

use super::error::WasmError;
use super::types::syn_type_to_wasm;
use std::collections::HashMap;
use walrus::ir::{BinaryOp, UnaryOp};
use walrus::{FunctionId, InstrSeqBuilder, LocalId, ValType};

/// Collected string literals with their memory offsets.
pub struct StringLiterals {
    strings: Vec<(u32, u32, String)>, // (offset, len, content)
    next_offset: u32,
}

impl StringLiterals {
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            next_offset: 0,
        }
    }

    /// Add a string literal and return its (offset, length).
    pub fn add(&mut self, s: &str) -> (u32, u32) {
        // Check if we already have this string
        for (offset, len, content) in &self.strings {
            if content == s {
                return (*offset, *len);
            }
        }

        let bytes = s.as_bytes();
        let offset = self.next_offset;
        let len = bytes.len() as u32;

        self.strings.push((offset, len, s.to_string()));
        self.next_offset += len;

        (offset, len)
    }

    /// Get all collected strings with their offsets.
    pub fn into_vec(self) -> Vec<(u32, u32, String)> {
        self.strings
    }
}

impl Default for StringLiterals {
    fn default() -> Self {
        Self::new()
    }
}

/// Context for compiling expressions within a function.
pub struct ExprContext<'a, 'b, 'c> {
    /// Map from variable names to local IDs
    pub locals: &'a HashMap<String, LocalId>,
    /// Map from function names to function IDs
    pub functions: &'a HashMap<String, FunctionId>,
    /// The current instruction sequence builder
    pub builder: &'b mut InstrSeqBuilder<'a>,
    /// String literals collector
    pub string_literals: &'c mut StringLiterals,
}

impl<'a, 'b, 'c> ExprContext<'a, 'b, 'c> {
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
            syn::Expr::Macro(mac) => self.compile_macro(mac),
            syn::Expr::While(while_expr) => self.compile_while(while_expr),
            syn::Expr::Loop(loop_expr) => self.compile_loop(loop_expr),
            syn::Expr::Tuple(tuple) => self.compile_tuple(tuple),
            other => Err(WasmError::Unsupported(format!(
                "Expression type: {}",
                quote::quote!(#other)
            ))),
        }
    }

    fn compile_macro(&mut self, mac: &syn::ExprMacro) -> Result<(), WasmError> {
        let macro_name = mac
            .mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();

        match macro_name.as_str() {
            "println" | "print" => {
                // Parse the macro tokens to extract arguments
                let tokens = mac.mac.tokens.clone();
                let tokens_str = tokens.to_string();

                // Check if it's a format string with arguments like "{}" , value
                // or "{} {}" , value1 , value2
                if tokens_str.contains(',') {
                    // Parse format string with arguments
                    // Format: "fmt_str" , arg1 , arg2 , ...
                    let parts: Vec<&str> = tokens_str.splitn(2, ',').collect();
                    if parts.len() == 2 {
                        let fmt_str = parts[0].trim();
                        let args_str = parts[1].trim();

                        // Count {} placeholders
                        let placeholder_count = fmt_str.matches("{}").count();

                        // Parse the arguments
                        let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();

                        if args.len() == placeholder_count {
                            // Print each argument
                            for arg in args {
                                if let Ok(expr) = syn::parse_str::<syn::Expr>(arg) {
                                    self.compile_expr(&expr)?;

                                    if let Some(&func_id) = self.functions.get("print_i32") {
                                        self.builder.call(func_id);
                                    } else {
                                        return Err(WasmError::UnknownIdentifier(
                                            "print_i32".to_string(),
                                        ));
                                    }
                                } else {
                                    return Err(WasmError::Unsupported(format!(
                                        "Cannot parse argument: {}",
                                        arg
                                    )));
                                }
                            }
                            return Ok(());
                        }
                    }
                }

                // Try to parse as a single expression (could be string literal or other expr)
                if let Ok(expr) = syn::parse2::<syn::Expr>(tokens) {
                    if let syn::Expr::Lit(lit_expr) = &expr
                        && let syn::Lit::Str(lit_str) = &lit_expr.lit
                    {
                        // String literal - store in memory and call print_str
                        let s = lit_str.value();
                        let (offset, len) = self.string_literals.add(&s);

                        // Push offset and length onto stack
                        self.builder.i32_const(offset as i32);
                        self.builder.i32_const(len as i32);

                        // Call print_str
                        if let Some(&func_id) = self.functions.get("print_str") {
                            self.builder.call(func_id);
                            return Ok(());
                        }
                        return Err(WasmError::UnknownIdentifier("print_str".to_string()));
                    }

                    // Not a string literal - try as integer expression
                    self.compile_expr(&expr)?;

                    // Call print_i32
                    if let Some(&func_id) = self.functions.get("print_i32") {
                        self.builder.call(func_id);
                        Ok(())
                    } else {
                        Err(WasmError::UnknownIdentifier("print_i32".to_string()))
                    }
                } else {
                    Err(WasmError::Unsupported(format!(
                        "Cannot parse println! arguments: {}",
                        tokens_str
                    )))
                }
            }
            _ => Err(WasmError::Unsupported(format!("Macro: {}!", macro_name))),
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
            _ => Err(WasmError::Unsupported(
                "Unsupported literal type".to_string(),
            )),
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
                )));
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
                )));
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

        // For now, inline simple println! calls in if blocks
        // Extract print function IDs before entering the closure
        let print_i32_id = self.functions.get("print_i32").copied();
        let print_str_id = self.functions.get("print_str").copied();
        let locals_clone = self.locals.clone();
        let functions_clone = self.functions.clone();

        // Collect what we need from then branch
        let then_stmts = &if_expr.then_branch.stmts;
        let else_branch = &if_expr.else_branch;

        // Use if-else structure
        self.builder.if_else(
            None, // No result type for statement-like if
            |then_builder| {
                // Compile then block - inline simple statements
                for stmt in then_stmts {
                    compile_stmt_inline(
                        stmt,
                        then_builder,
                        &locals_clone,
                        &functions_clone,
                        print_i32_id,
                        print_str_id,
                    );
                }
            },
            |else_builder| {
                // Compile else block if present
                if let Some((_, else_expr)) = else_branch {
                    match else_expr.as_ref() {
                        syn::Expr::Block(block) => {
                            for stmt in &block.block.stmts {
                                compile_stmt_inline(
                                    stmt,
                                    else_builder,
                                    &locals_clone,
                                    &functions_clone,
                                    print_i32_id,
                                    print_str_id,
                                );
                            }
                        }
                        syn::Expr::If(nested_if) => {
                            // Nested if - compile condition and recurse
                            compile_if_inline(
                                nested_if,
                                else_builder,
                                &locals_clone,
                                &functions_clone,
                                print_i32_id,
                                print_str_id,
                            );
                        }
                        _ => {}
                    }
                }
            },
        );

        Ok(())
    }

    fn compile_while(&mut self, while_expr: &syn::ExprWhile) -> Result<(), WasmError> {
        // Extract function IDs before entering the closure
        let print_i32_id = self.functions.get("print_i32").copied();
        let print_str_id = self.functions.get("print_str").copied();
        let locals_clone = self.locals.clone();
        let functions_clone = self.functions.clone();
        let cond = &while_expr.cond;
        let body_stmts = &while_expr.body.stmts;

        // WASM loop structure: loop { if (cond) { body; continue } else { break } }
        self.builder.loop_(None, |loop_builder| {
            // Get loop id before entering nested closures
            let loop_id = loop_builder.id();

            // Compile condition
            compile_expr_inline(
                cond,
                loop_builder,
                &locals_clone,
                &functions_clone,
                print_i32_id,
                print_str_id,
            );

            // if condition { body; br loop } else { break }
            loop_builder.if_else(
                None,
                |then_builder| {
                    // Compile body
                    for stmt in body_stmts {
                        compile_stmt_inline(
                            stmt,
                            then_builder,
                            &locals_clone,
                            &functions_clone,
                            print_i32_id,
                            print_str_id,
                        );
                    }
                    // Branch back to loop start
                    then_builder.br(loop_id);
                },
                |_else_builder| {
                    // Exit loop (implicit)
                },
            );
        });

        Ok(())
    }

    fn compile_loop(&mut self, loop_expr: &syn::ExprLoop) -> Result<(), WasmError> {
        let print_i32_id = self.functions.get("print_i32").copied();
        let print_str_id = self.functions.get("print_str").copied();
        let locals_clone = self.locals.clone();
        let functions_clone = self.functions.clone();
        let body_stmts = &loop_expr.body.stmts;

        self.builder.loop_(None, |loop_builder| {
            let loop_id = loop_builder.id();
            for stmt in body_stmts {
                compile_stmt_inline(
                    stmt,
                    loop_builder,
                    &locals_clone,
                    &functions_clone,
                    print_i32_id,
                    print_str_id,
                );
            }
            // Infinite loop - branch back
            loop_builder.br(loop_id);
        });

        Ok(())
    }

    fn compile_tuple(&mut self, tuple: &syn::ExprTuple) -> Result<(), WasmError> {
        // Tuples don't have a direct WASM representation
        // This is typically used in tuple assignments which are handled specially
        // For now, just compile each element (useful for expressions like (a, b) = (b, a+b))
        for elem in &tuple.elems {
            self.compile_expr(elem)?;
        }
        Ok(())
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
            _ => return Err(WasmError::TypeError(format!("Cannot cast to {target:?}"))),
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
            syn::Stmt::Macro(stmt_macro) => {
                // Convert macro statement to expression and compile
                let mac_expr = syn::ExprMacro {
                    attrs: stmt_macro.attrs.clone(),
                    mac: stmt_macro.mac.clone(),
                };
                self.compile_expr(&syn::Expr::Macro(mac_expr))
            }
        }
    }
}

// Helper functions for inline compilation within closures
// These are needed because we can't pass the full ExprContext into closure builders

fn compile_stmt_inline(
    stmt: &syn::Stmt,
    builder: &mut InstrSeqBuilder<'_>,
    locals: &HashMap<String, LocalId>,
    functions: &HashMap<String, FunctionId>,
    print_i32_id: Option<FunctionId>,
    print_str_id: Option<FunctionId>,
) {
    match stmt {
        syn::Stmt::Expr(expr, _) => {
            compile_expr_inline(expr, builder, locals, functions, print_i32_id, print_str_id);
        }
        syn::Stmt::Local(local) => {
            // Handle local variable initialization
            if let Some(init) = &local.init {
                compile_pat_assignment_inline(
                    &local.pat,
                    &init.expr,
                    builder,
                    locals,
                    functions,
                    print_i32_id,
                    print_str_id,
                );
            }
        }
        syn::Stmt::Macro(stmt_macro) => {
            let mac_expr = syn::ExprMacro {
                attrs: stmt_macro.attrs.clone(),
                mac: stmt_macro.mac.clone(),
            };
            compile_expr_inline(
                &syn::Expr::Macro(mac_expr),
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );
        }
        _ => {}
    }
}

fn compile_pat_assignment_inline(
    pat: &syn::Pat,
    expr: &syn::Expr,
    builder: &mut InstrSeqBuilder<'_>,
    locals: &HashMap<String, LocalId>,
    functions: &HashMap<String, FunctionId>,
    print_i32_id: Option<FunctionId>,
    print_str_id: Option<FunctionId>,
) {
    match pat {
        syn::Pat::Ident(pat_ident) => {
            compile_expr_inline(expr, builder, locals, functions, print_i32_id, print_str_id);
            let name = pat_ident.ident.to_string();
            if let Some(&local_id) = locals.get(&name) {
                builder.local_set(local_id);
            }
        }
        syn::Pat::Type(pat_type) => {
            if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                compile_expr_inline(expr, builder, locals, functions, print_i32_id, print_str_id);
                let name = pat_ident.ident.to_string();
                if let Some(&local_id) = locals.get(&name) {
                    builder.local_set(local_id);
                }
            }
        }
        syn::Pat::Tuple(pat_tuple) => {
            // Handle tuple pattern: let (a, b) = (expr1, expr2)
            if let syn::Expr::Tuple(expr_tuple) = expr {
                for (elem_pat, elem_expr) in pat_tuple.elems.iter().zip(expr_tuple.elems.iter()) {
                    compile_pat_assignment_inline(
                        elem_pat,
                        elem_expr,
                        builder,
                        locals,
                        functions,
                        print_i32_id,
                        print_str_id,
                    );
                }
            }
        }
        _ => {}
    }
}

fn compile_expr_inline(
    expr: &syn::Expr,
    builder: &mut InstrSeqBuilder<'_>,
    locals: &HashMap<String, LocalId>,
    functions: &HashMap<String, FunctionId>,
    print_i32_id: Option<FunctionId>,
    print_str_id: Option<FunctionId>,
) {
    match expr {
        syn::Expr::Lit(lit) => match &lit.lit {
            syn::Lit::Int(int) => {
                if let Ok(value) = int.base10_parse::<i64>() {
                    builder.i32_const(value as i32);
                }
            }
            syn::Lit::Bool(b) => {
                builder.i32_const(if b.value { 1 } else { 0 });
            }
            _ => {}
        },
        syn::Expr::Binary(binary) => {
            // Check for compound assignment operators (+=, -=, etc.)
            match &binary.op {
                syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_) => {
                    // Get the variable name
                    if let syn::Expr::Path(path) = binary.left.as_ref() {
                        if path.path.segments.len() == 1 {
                            let name = path.path.segments[0].ident.to_string();
                            if let Some(&local_id) = locals.get(&name) {
                                // Load current value
                                builder.local_get(local_id);
                                // Compile right side
                                compile_expr_inline(
                                    &binary.right,
                                    builder,
                                    locals,
                                    functions,
                                    print_i32_id,
                                    print_str_id,
                                );
                                // Apply operation
                                let op = match &binary.op {
                                    syn::BinOp::AddAssign(_) => BinaryOp::I32Add,
                                    syn::BinOp::SubAssign(_) => BinaryOp::I32Sub,
                                    syn::BinOp::MulAssign(_) => BinaryOp::I32Mul,
                                    syn::BinOp::DivAssign(_) => BinaryOp::I32DivS,
                                    _ => return,
                                };
                                builder.binop(op);
                                // Store result
                                builder.local_set(local_id);
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }

            // Regular binary operations
            compile_expr_inline(
                &binary.left,
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );
            compile_expr_inline(
                &binary.right,
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );

            let op = match &binary.op {
                syn::BinOp::Add(_) => BinaryOp::I32Add,
                syn::BinOp::Sub(_) => BinaryOp::I32Sub,
                syn::BinOp::Mul(_) => BinaryOp::I32Mul,
                syn::BinOp::Div(_) => BinaryOp::I32DivS,
                syn::BinOp::Rem(_) => BinaryOp::I32RemS,
                syn::BinOp::Eq(_) => BinaryOp::I32Eq,
                syn::BinOp::Ne(_) => BinaryOp::I32Ne,
                syn::BinOp::Lt(_) => BinaryOp::I32LtS,
                syn::BinOp::Le(_) => BinaryOp::I32LeS,
                syn::BinOp::Gt(_) => BinaryOp::I32GtS,
                syn::BinOp::Ge(_) => BinaryOp::I32GeS,
                _ => return,
            };
            builder.binop(op);
        }
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            if let Some(&local_id) = locals.get(&name) {
                builder.local_get(local_id);
            }
        }
        syn::Expr::Paren(paren) => {
            compile_expr_inline(
                &paren.expr,
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );
        }
        syn::Expr::Assign(assign) => {
            compile_expr_inline(
                &assign.right,
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );
            if let syn::Expr::Path(path) = assign.left.as_ref() {
                if path.path.segments.len() == 1 {
                    let name = path.path.segments[0].ident.to_string();
                    if let Some(&local_id) = locals.get(&name) {
                        builder.local_set(local_id);
                    }
                }
            }
        }
        syn::Expr::Call(call) => {
            // Compile arguments
            for arg in &call.args {
                compile_expr_inline(arg, builder, locals, functions, print_i32_id, print_str_id);
            }
            // Get function name and call
            if let syn::Expr::Path(path) = call.func.as_ref() {
                if path.path.segments.len() == 1 {
                    let func_name = path.path.segments[0].ident.to_string();
                    if let Some(&func_id) = functions.get(&func_name) {
                        builder.call(func_id);
                    }
                }
            }
        }
        syn::Expr::Macro(mac) => {
            let macro_name = mac
                .mac
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();

            if macro_name == "println" || macro_name == "print" {
                let tokens = mac.mac.tokens.clone();
                let tokens_str = tokens.to_string();

                // Handle format string with arguments
                if tokens_str.contains(',') {
                    let parts: Vec<&str> = tokens_str.splitn(2, ',').collect();
                    if parts.len() == 2 {
                        let args_str = parts[1].trim();
                        let args: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();

                        for arg in args {
                            if let Ok(arg_expr) = syn::parse_str::<syn::Expr>(arg) {
                                compile_expr_inline(
                                    &arg_expr,
                                    builder,
                                    locals,
                                    functions,
                                    print_i32_id,
                                    print_str_id,
                                );
                                if let Some(func_id) = print_i32_id {
                                    builder.call(func_id);
                                }
                            }
                        }
                    }
                } else if let Ok(inner_expr) = syn::parse2::<syn::Expr>(tokens) {
                    // Check if it's a string literal
                    if let syn::Expr::Lit(lit_expr) = &inner_expr {
                        if let syn::Lit::Str(_) = &lit_expr.lit {
                            // String literals in inline context - we can't easily handle them
                            // because we don't have access to string_literals collector.
                            // For now, just skip (produces no output)
                            // A proper fix would pass string_literals through
                            return;
                        }
                    }
                    // Not a string literal - compile as integer expression
                    compile_expr_inline(
                        &inner_expr,
                        builder,
                        locals,
                        functions,
                        print_i32_id,
                        print_str_id,
                    );
                    if let Some(func_id) = print_i32_id {
                        builder.call(func_id);
                    }
                }
            }
        }
        syn::Expr::If(if_expr) => {
            compile_if_inline(
                if_expr,
                builder,
                locals,
                functions,
                print_i32_id,
                print_str_id,
            );
        }
        syn::Expr::Block(block) => {
            for stmt in &block.block.stmts {
                compile_stmt_inline(stmt, builder, locals, functions, print_i32_id, print_str_id);
            }
        }
        syn::Expr::Return(ret) => {
            if let Some(expr) = &ret.expr {
                compile_expr_inline(expr, builder, locals, functions, print_i32_id, print_str_id);
            }
            builder.return_();
        }
        _ => {}
    }
}

fn compile_if_inline(
    if_expr: &syn::ExprIf,
    builder: &mut InstrSeqBuilder<'_>,
    locals: &HashMap<String, LocalId>,
    functions: &HashMap<String, FunctionId>,
    print_i32_id: Option<FunctionId>,
    print_str_id: Option<FunctionId>,
) {
    // Compile condition
    compile_expr_inline(
        &if_expr.cond,
        builder,
        locals,
        functions,
        print_i32_id,
        print_str_id,
    );

    let then_stmts = &if_expr.then_branch.stmts;
    let else_branch = &if_expr.else_branch;
    let locals = locals.clone();
    let functions = functions.clone();

    builder.if_else(
        None,
        |then_builder| {
            for stmt in then_stmts {
                compile_stmt_inline(
                    stmt,
                    then_builder,
                    &locals,
                    &functions,
                    print_i32_id,
                    print_str_id,
                );
            }
        },
        |else_builder| {
            if let Some((_, else_expr)) = else_branch {
                match else_expr.as_ref() {
                    syn::Expr::Block(block) => {
                        for stmt in &block.block.stmts {
                            compile_stmt_inline(
                                stmt,
                                else_builder,
                                &locals,
                                &functions,
                                print_i32_id,
                                print_str_id,
                            );
                        }
                    }
                    syn::Expr::If(nested_if) => {
                        compile_if_inline(
                            nested_if,
                            else_builder,
                            &locals,
                            &functions,
                            print_i32_id,
                            print_str_id,
                        );
                    }
                    _ => {}
                }
            }
        },
    );
}
