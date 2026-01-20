//! WASM compiler - transforms syn::File to WASM binary using wasm-encoder.

use super::error::{suggest_alternative, WasmError, SUPPORTED_FUNCTIONS};
use super::types::{infer_expr_type, infer_lit_type, FunctionSig, TypeContext, WasmType};
use crate::codegen::{CodegenError, Mapping, SourceMap};
use wasm_encoder::{
    CodeSection, DataSection, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemorySection, MemoryType, Module, TypeSection, ValType,
};

/// Get a string representation of a binary operator.
fn binop_to_string(op: &syn::BinOp) -> &'static str {
    match op {
        syn::BinOp::Add(_) => "+",
        syn::BinOp::Sub(_) => "-",
        syn::BinOp::Mul(_) => "*",
        syn::BinOp::Div(_) => "/",
        syn::BinOp::Rem(_) => "%",
        syn::BinOp::And(_) => "&&",
        syn::BinOp::Or(_) => "||",
        syn::BinOp::BitXor(_) => "^",
        syn::BinOp::BitAnd(_) => "&",
        syn::BinOp::BitOr(_) => "|",
        syn::BinOp::Shl(_) => "<<",
        syn::BinOp::Shr(_) => ">>",
        syn::BinOp::Eq(_) => "==",
        syn::BinOp::Lt(_) => "<",
        syn::BinOp::Le(_) => "<=",
        syn::BinOp::Ne(_) => "!=",
        syn::BinOp::Ge(_) => ">=",
        syn::BinOp::Gt(_) => ">",
        syn::BinOp::AddAssign(_) => "+=",
        syn::BinOp::SubAssign(_) => "-=",
        syn::BinOp::MulAssign(_) => "*=",
        syn::BinOp::DivAssign(_) => "/=",
        syn::BinOp::RemAssign(_) => "%=",
        syn::BinOp::BitXorAssign(_) => "^=",
        syn::BinOp::BitAndAssign(_) => "&=",
        syn::BinOp::BitOrAssign(_) => "|=",
        syn::BinOp::ShlAssign(_) => "<<=",
        syn::BinOp::ShrAssign(_) => ">>=",
        _ => "?",
    }
}

/// Get a string representation of a unary operator.
fn unop_to_string(op: &syn::UnOp) -> &'static str {
    match op {
        syn::UnOp::Deref(_) => "*",
        syn::UnOp::Not(_) => "!",
        syn::UnOp::Neg(_) => "-",
        _ => "?",
    }
}

/// String data to be stored in the data section.
#[derive(Debug)]
struct StringData {
    /// Offset in linear memory
    offset: u32,
    /// The string content (UTF-8 bytes)
    content: String,
    /// Source location for comments
    source_line: Option<u32>,
    /// Source context (e.g., "fmt.Println(...)")
    source_context: Option<String>,
}

/// Compiled function data.
struct CompiledFunction {
    /// Function name
    name: String,
    /// Parameter types
    params: Vec<ValType>,
    /// Result types
    results: Vec<ValType>,
    /// Local variable types (excluding params)
    locals: Vec<ValType>,
    /// Instructions
    instructions: Vec<Instruction<'static>>,
    /// Whether to export this function
    export: bool,
}

/// WASM compiler state.
pub struct WasmCompiler {
    /// Type context for tracking types
    type_ctx: TypeContext,
    /// Collected string data for the data section
    strings: Vec<StringData>,
    /// Current offset in the data section
    data_offset: u32,
    /// Source file name for source maps
    source_file: Option<String>,
    /// Go source code content (for error context)
    source_content: Option<String>,
    /// Source map mappings
    mappings: Vec<Mapping>,
    /// Current output line number
    current_output_line: u32,
    /// Current input line number (from Go source)
    current_input_line: u32,
    /// Current input column
    current_input_column: u32,
    /// Function being compiled
    current_function: Option<String>,
    /// Stack of (label, block_depth_at_loop_start) for break/continue
    /// The block_depth_at_loop_start allows us to calculate relative depth
    loop_stack: Vec<(Option<String>, u32)>,
    /// Block depth for br instructions
    block_depth: u32,
    /// Current function's instructions being built
    current_instructions: Vec<Instruction<'static>>,
    /// Current function's local names and types
    current_locals: Vec<(String, ValType)>,
    /// Number of parameters in current function
    current_param_count: u32,
}

impl WasmCompiler {
    /// Create a new WASM compiler.
    pub fn new() -> Self {
        Self {
            type_ctx: TypeContext::new(),
            strings: Vec::new(),
            data_offset: 0,
            source_file: None,
            source_content: None,
            mappings: Vec::new(),
            current_output_line: 1,
            current_input_line: 1,
            current_input_column: 1,
            current_function: None,
            loop_stack: Vec::new(),
            block_depth: 0,
            current_instructions: Vec::new(),
            current_locals: Vec::new(),
            current_param_count: 0,
        }
    }

    /// Set the source file name for source mapping.
    pub fn set_source_file(&mut self, name: &str) {
        self.source_file = Some(name.to_string());
    }

    /// Set the source content for error messages.
    pub fn set_source_content(&mut self, content: &str) {
        self.source_content = Some(content.to_string());
    }

    /// Get the source map after compilation.
    pub fn into_source_map(self) -> SourceMap {
        let mut sm = SourceMap::new(self.source_file.as_deref().unwrap_or("input.go"));
        sm.mappings = self.mappings;
        sm
    }

    /// Add a mapping from input to output position.
    fn add_mapping(&mut self, name: Option<String>) {
        self.mappings.push(Mapping {
            input_line: self.current_input_line,
            input_column: self.current_input_column,
            output_line: self.current_output_line,
            output_column: 1,
            name,
        });
    }

    /// Store a string and return its (offset, length).
    fn store_string(
        &mut self,
        s: &str,
        source_line: Option<u32>,
        source_context: Option<String>,
    ) -> (u32, u32) {
        let offset = self.data_offset;
        let len = s.len() as u32;
        self.strings.push(StringData {
            offset,
            content: s.to_string(),
            source_line,
            source_context,
        });
        self.data_offset += len;
        (offset, len)
    }

    /// Convert WasmType to wasm-encoder ValType.
    fn wasm_type_to_valtype(ty: WasmType) -> ValType {
        match ty {
            WasmType::I32 | WasmType::Ptr => ValType::I32,
            WasmType::I64 => ValType::I64,
            WasmType::F32 => ValType::F32,
            WasmType::F64 => ValType::F64,
            WasmType::Void => ValType::I32, // Shouldn't be used for Void
        }
    }

    /// Emit an instruction.
    fn emit(&mut self, instr: Instruction<'static>) {
        self.current_instructions.push(instr);
    }

    /// Get local index by name.
    fn get_local_index(&self, name: &str) -> Option<u32> {
        self.current_locals
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| i as u32)
    }

    /// Compile a syn::File to WAT string.
    pub fn compile(&mut self, file: syn::File) -> Result<String, CodegenError> {
        // Build WASM binary
        let wasm_bytes = self.compile_to_bytes(file)?;

        // Convert to WAT using wasmprinter
        let wat = wasmprinter::print_bytes(&wasm_bytes)
            .map_err(|e| CodegenError::generation(format!("Failed to print WAT: {}", e)))?;

        Ok(wat)
    }

    /// Compile a syn::File to WASM binary.
    pub fn compile_to_bytes(&mut self, file: syn::File) -> Result<Vec<u8>, CodegenError> {
        let mut module = Module::new();

        // First pass: collect function signatures
        for item in &file.items {
            if let syn::Item::Fn(func) = item {
                let name = func.sig.ident.to_string();
                let sig = self.extract_function_sig(&func.sig);
                self.type_ctx.define_function(&name, sig);
            }
        }

        // Compile functions
        let mut compiled_functions = Vec::new();
        for item in file.items {
            if let syn::Item::Fn(func) = item {
                let compiled = self.compile_function(func)?;
                compiled_functions.push(compiled);
            }
        }

        // Build type section
        // Type 0: print function (i32, i32) -> ()
        // Type 1+: user functions
        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32, ValType::I32], vec![]); // print type

        let mut func_type_indices = Vec::new();
        for func in &compiled_functions {
            let type_idx = types.len();
            types.ty().function(func.params.clone(), func.results.clone());
            func_type_indices.push(type_idx);
        }
        module.section(&types);

        // Import section
        let mut imports = ImportSection::new();
        imports.import("env", "print", wasm_encoder::EntityType::Function(0));
        module.section(&imports);

        // Function section (type indices for each function)
        let mut functions = FunctionSection::new();
        for &type_idx in &func_type_indices {
            functions.function(type_idx);
        }
        module.section(&functions);

        // Memory section
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section
        let mut exports = ExportSection::new();
        exports.export("memory", ExportKind::Memory, 0);

        for (i, func) in compiled_functions.iter().enumerate() {
            if func.export {
                // Function index = import count (1 for print) + function index
                exports.export(&func.name, ExportKind::Func, (i + 1) as u32);
            }
        }
        module.section(&exports);

        // Code section
        let mut codes = CodeSection::new();
        for func in &compiled_functions {
            let mut f = Function::new(
                func.locals
                    .iter()
                    .map(|t| (1, *t))
                    .collect::<Vec<_>>(),
            );
            for instr in &func.instructions {
                f.instruction(instr);
            }
            codes.function(&f);
        }
        module.section(&codes);

        // Data section
        if !self.strings.is_empty() {
            let mut data = DataSection::new();
            for string_data in &self.strings {
                data.active(
                    0, // memory index
                    &wasm_encoder::ConstExpr::i32_const(string_data.offset as i32),
                    string_data.content.as_bytes().iter().copied(),
                );
            }
            module.section(&data);
        }

        Ok(module.finish())
    }

    /// Extract function signature from syn.
    fn extract_function_sig(&self, sig: &syn::Signature) -> FunctionSig {
        let mut params = Vec::new();
        for arg in &sig.inputs {
            if let syn::FnArg::Typed(pat_type) = arg {
                let ty = self.type_from_syn(&pat_type.ty);
                params.push(ty);
            }
        }

        let result = match &sig.output {
            syn::ReturnType::Default => WasmType::Void,
            syn::ReturnType::Type(_, ty) => self.type_from_syn(ty),
        };

        FunctionSig { params, result }
    }

    /// Convert syn::Type to WasmType.
    fn type_from_syn(&self, ty: &syn::Type) -> WasmType {
        if let syn::Type::Path(type_path) = ty {
            if let Some(ident) = type_path.path.get_ident() {
                return WasmType::from_rust_type(&ident.to_string());
            }
        }
        WasmType::I32
    }

    /// Compile a function.
    fn compile_function(&mut self, func: syn::ItemFn) -> Result<CompiledFunction, CodegenError> {
        let name = func.sig.ident.to_string();
        self.current_function = Some(name.clone());
        self.type_ctx.clear_locals();
        self.loop_stack.clear();
        self.block_depth = 0;
        self.current_instructions.clear();
        self.current_locals.clear();

        // Estimate source line (heuristic: main starts around line 5)
        let estimated_go_line = if name == "main" { 5 } else { 10 };
        self.current_input_line = estimated_go_line;

        // Process parameters
        let sig = self.extract_function_sig(&func.sig);
        let mut params = Vec::new();

        for (i, arg) in func.sig.inputs.iter().enumerate() {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat_ident) = pat_type.pat.as_ref() {
                    let param_name = pat_ident.ident.to_string();
                    let ty = sig.params[i];
                    let valtype = Self::wasm_type_to_valtype(ty);
                    self.type_ctx.define_local(&param_name, ty);
                    self.current_locals.push((param_name, valtype));
                    params.push(valtype);
                }
            }
        }

        self.current_param_count = params.len() as u32;

        // Result type
        let results = if sig.result != WasmType::Void {
            vec![Self::wasm_type_to_valtype(sig.result)]
        } else {
            vec![]
        };

        self.add_mapping(Some(name.clone()));

        // Compile function body
        self.compile_block(&func.block)?;

        // Add end instruction
        self.emit(Instruction::End);

        // Collect locals (excluding params)
        let locals: Vec<ValType> = self.current_locals[params.len()..]
            .iter()
            .map(|(_, t)| *t)
            .collect();

        let is_exported = matches!(func.vis, syn::Visibility::Public(_)) || name == "main";

        self.current_function = None;

        Ok(CompiledFunction {
            name,
            params,
            results,
            locals,
            instructions: std::mem::take(&mut self.current_instructions),
            export: is_exported,
        })
    }

    /// Compile a block.
    fn compile_block(&mut self, block: &syn::Block) -> Result<(), CodegenError> {
        for stmt in &block.stmts {
            self.compile_stmt(stmt)?;
        }
        Ok(())
    }

    /// Compile a statement.
    fn compile_stmt(&mut self, stmt: &syn::Stmt) -> Result<(), CodegenError> {
        match stmt {
            syn::Stmt::Local(local) => self.compile_local(local),
            syn::Stmt::Expr(expr, semi) => {
                self.compile_expr(expr)?;
                // If there's a semicolon and the expression produces a value, drop it
                if semi.is_some()
                    && !matches!(
                        expr,
                        syn::Expr::If(_)
                            | syn::Expr::While(_)
                            | syn::Expr::Loop(_)
                            | syn::Expr::Block(_)
                    )
                {
                    let ty = infer_expr_type(expr, &self.type_ctx);
                    if ty != WasmType::Void {
                        self.emit(Instruction::Drop);
                    }
                }
                Ok(())
            }
            syn::Stmt::Item(_) => Ok(()), // Items in blocks not supported
            syn::Stmt::Macro(macro_stmt) => self.compile_macro_stmt(&macro_stmt.mac),
        }
    }

    /// Compile a local variable declaration.
    fn compile_local(&mut self, local: &syn::Local) -> Result<(), CodegenError> {
        // Get the variable name
        let name = if let syn::Pat::Ident(pat_ident) = &local.pat {
            pat_ident.ident.to_string()
        } else {
            return Err(CodegenError::unsupported(
                "complex pattern in let binding",
                self.current_input_line,
                self.current_input_column,
            ));
        };

        // Infer type from initializer or annotation
        let ty = if let Some(init) = &local.init {
            infer_expr_type(&init.expr, &self.type_ctx)
        } else {
            WasmType::I32
        };

        // Define the local
        let valtype = Self::wasm_type_to_valtype(ty);
        self.type_ctx.define_local(&name, ty);
        let local_idx = self.current_locals.len() as u32;
        self.current_locals.push((name.clone(), valtype));

        // Compile initializer if present
        if let Some(init) = &local.init {
            self.compile_expr(&init.expr)?;
            self.emit(Instruction::LocalSet(local_idx));
        }

        Ok(())
    }

    /// Compile an expression.
    fn compile_expr(&mut self, expr: &syn::Expr) -> Result<(), CodegenError> {
        match expr {
            syn::Expr::Lit(lit) => self.compile_lit(&lit.lit),
            syn::Expr::Path(path) => self.compile_path(path),
            syn::Expr::Binary(binary) => self.compile_binary(binary),
            syn::Expr::Unary(unary) => self.compile_unary(unary),
            syn::Expr::Paren(paren) => self.compile_expr(&paren.expr),
            syn::Expr::Call(call) => self.compile_call(call),
            syn::Expr::Macro(mac) => self.compile_macro(&mac.mac),
            syn::Expr::If(if_expr) => self.compile_if(if_expr),
            syn::Expr::While(while_expr) => self.compile_while(while_expr),
            syn::Expr::Loop(loop_expr) => self.compile_loop(loop_expr),
            syn::Expr::Block(block) => {
                self.emit(Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.compile_block(&block.block)?;
                self.block_depth -= 1;
                self.emit(Instruction::End);
                Ok(())
            }
            syn::Expr::Return(ret) => self.compile_return(ret),
            syn::Expr::Break(brk) => self.compile_break(brk),
            syn::Expr::Continue(cont) => self.compile_continue(cont),
            syn::Expr::Assign(assign) => self.compile_assign(assign),
            syn::Expr::Cast(cast) => self.compile_cast(cast),
            syn::Expr::Index(index) => self.compile_index(index),
            _ => Err(CodegenError::unsupported(
                format!("expression type: {:?}", std::mem::discriminant(expr)),
                self.current_input_line,
                self.current_input_column,
            )),
        }
    }

    /// Compile a literal.
    fn compile_lit(&mut self, lit: &syn::Lit) -> Result<(), CodegenError> {
        match lit {
            syn::Lit::Int(int_lit) => {
                let ty = infer_lit_type(lit);
                let value: i64 = int_lit
                    .base10_parse()
                    .map_err(|e| CodegenError::generation(e.to_string()))?;
                match ty {
                    WasmType::I32 | WasmType::Ptr => self.emit(Instruction::I32Const(value as i32)),
                    WasmType::I64 => self.emit(Instruction::I64Const(value)),
                    _ => self.emit(Instruction::I32Const(value as i32)),
                }
            }
            syn::Lit::Float(float_lit) => {
                let ty = infer_lit_type(lit);
                let value: f64 = float_lit
                    .base10_parse()
                    .map_err(|e| CodegenError::generation(e.to_string()))?;
                match ty {
                    WasmType::F32 => self.emit(Instruction::F32Const(value as f32)),
                    WasmType::F64 => self.emit(Instruction::F64Const(value)),
                    _ => self.emit(Instruction::F64Const(value)),
                }
            }
            syn::Lit::Bool(bool_lit) => {
                let value = if bool_lit.value { 1 } else { 0 };
                self.emit(Instruction::I32Const(value));
            }
            syn::Lit::Str(str_lit) => {
                // Store string and push pointer
                let (offset, _len) = self.store_string(&str_lit.value(), None, None);
                self.emit(Instruction::I32Const(offset as i32));
            }
            syn::Lit::Char(char_lit) => {
                let value = char_lit.value() as i32;
                self.emit(Instruction::I32Const(value));
            }
            _ => {
                return Err(CodegenError::unsupported(
                    "unsupported literal type",
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        }
        Ok(())
    }

    /// Compile a path (variable reference).
    fn compile_path(&mut self, path: &syn::ExprPath) -> Result<(), CodegenError> {
        if let Some(ident) = path.path.get_ident() {
            let name = ident.to_string();
            if let Some(idx) = self.get_local_index(&name) {
                self.emit(Instruction::LocalGet(idx));
                Ok(())
            } else {
                Err(CodegenError::unsupported(
                    format!("undefined variable: {}", name),
                    self.current_input_line,
                    self.current_input_column,
                ))
            }
        } else {
            Ok(())
        }
    }

    /// Check if an operator is a compound assignment operator.
    fn is_assign_op(op: &syn::BinOp) -> bool {
        matches!(
            op,
            syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_)
        )
    }

    /// Compile a compound assignment expression (+=, -=, etc.)
    fn compile_compound_assign(&mut self, binary: &syn::ExprBinary) -> Result<(), CodegenError> {
        // Get the variable name from the left side
        let name = if let syn::Expr::Path(path) = binary.left.as_ref() {
            if let Some(ident) = path.path.get_ident() {
                ident.to_string()
            } else {
                return Err(CodegenError::unsupported(
                    "complex assignment target",
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        } else {
            return Err(CodegenError::unsupported(
                "complex assignment target",
                self.current_input_line,
                self.current_input_column,
            ));
        };

        // Get the local index
        let idx = if let Some(idx) = self.get_local_index(&name) {
            idx
        } else {
            return Err(CodegenError::unsupported(
                format!("undefined variable: {}", name),
                self.current_input_line,
                self.current_input_column,
            ));
        };

        // Get the operand type
        let ty = infer_expr_type(&binary.left, &self.type_ctx);

        // Load current value
        self.emit(Instruction::LocalGet(idx));

        // Compile the right side value
        self.compile_expr(&binary.right)?;

        // Emit the operation
        match (&binary.op, ty) {
            (syn::BinOp::AddAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Add)
            }
            (syn::BinOp::AddAssign(_), WasmType::I64) => self.emit(Instruction::I64Add),
            (syn::BinOp::AddAssign(_), WasmType::F32) => self.emit(Instruction::F32Add),
            (syn::BinOp::AddAssign(_), WasmType::F64) => self.emit(Instruction::F64Add),

            (syn::BinOp::SubAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Sub)
            }
            (syn::BinOp::SubAssign(_), WasmType::I64) => self.emit(Instruction::I64Sub),
            (syn::BinOp::SubAssign(_), WasmType::F32) => self.emit(Instruction::F32Sub),
            (syn::BinOp::SubAssign(_), WasmType::F64) => self.emit(Instruction::F64Sub),

            (syn::BinOp::MulAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Mul)
            }
            (syn::BinOp::MulAssign(_), WasmType::I64) => self.emit(Instruction::I64Mul),
            (syn::BinOp::MulAssign(_), WasmType::F32) => self.emit(Instruction::F32Mul),
            (syn::BinOp::MulAssign(_), WasmType::F64) => self.emit(Instruction::F64Mul),

            (syn::BinOp::DivAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32DivS)
            }
            (syn::BinOp::DivAssign(_), WasmType::I64) => self.emit(Instruction::I64DivS),
            (syn::BinOp::DivAssign(_), WasmType::F32) => self.emit(Instruction::F32Div),
            (syn::BinOp::DivAssign(_), WasmType::F64) => self.emit(Instruction::F64Div),

            (syn::BinOp::RemAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32RemS)
            }
            (syn::BinOp::RemAssign(_), WasmType::I64) => self.emit(Instruction::I64RemS),

            (syn::BinOp::BitXorAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Xor)
            }
            (syn::BinOp::BitXorAssign(_), WasmType::I64) => self.emit(Instruction::I64Xor),

            (syn::BinOp::BitAndAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32And)
            }
            (syn::BinOp::BitAndAssign(_), WasmType::I64) => self.emit(Instruction::I64And),

            (syn::BinOp::BitOrAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Or)
            }
            (syn::BinOp::BitOrAssign(_), WasmType::I64) => self.emit(Instruction::I64Or),

            (syn::BinOp::ShlAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32Shl)
            }
            (syn::BinOp::ShlAssign(_), WasmType::I64) => self.emit(Instruction::I64Shl),

            (syn::BinOp::ShrAssign(_), WasmType::I32 | WasmType::Ptr) => {
                self.emit(Instruction::I32ShrS)
            }
            (syn::BinOp::ShrAssign(_), WasmType::I64) => self.emit(Instruction::I64ShrS),

            _ => {
                let op_str = binop_to_string(&binary.op);
                return Err(CodegenError::unsupported(
                    format!("unsupported compound assignment operator: {}", op_str),
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        }

        // Store the result back
        self.emit(Instruction::LocalSet(idx));

        Ok(())
    }

    /// Compile a binary expression.
    fn compile_binary(&mut self, binary: &syn::ExprBinary) -> Result<(), CodegenError> {
        // Handle compound assignment operators specially
        if Self::is_assign_op(&binary.op) {
            return self.compile_compound_assign(binary);
        }

        // Compile operands
        self.compile_expr(&binary.left)?;
        self.compile_expr(&binary.right)?;

        // Get operand type for instruction selection
        let ty = infer_expr_type(&binary.left, &self.type_ctx);

        // Emit operator
        match (&binary.op, ty) {
            (syn::BinOp::Add(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Add),
            (syn::BinOp::Add(_), WasmType::I64) => self.emit(Instruction::I64Add),
            (syn::BinOp::Add(_), WasmType::F32) => self.emit(Instruction::F32Add),
            (syn::BinOp::Add(_), WasmType::F64) => self.emit(Instruction::F64Add),

            (syn::BinOp::Sub(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Sub),
            (syn::BinOp::Sub(_), WasmType::I64) => self.emit(Instruction::I64Sub),
            (syn::BinOp::Sub(_), WasmType::F32) => self.emit(Instruction::F32Sub),
            (syn::BinOp::Sub(_), WasmType::F64) => self.emit(Instruction::F64Sub),

            (syn::BinOp::Mul(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Mul),
            (syn::BinOp::Mul(_), WasmType::I64) => self.emit(Instruction::I64Mul),
            (syn::BinOp::Mul(_), WasmType::F32) => self.emit(Instruction::F32Mul),
            (syn::BinOp::Mul(_), WasmType::F64) => self.emit(Instruction::F64Mul),

            (syn::BinOp::Div(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32DivS),
            (syn::BinOp::Div(_), WasmType::I64) => self.emit(Instruction::I64DivS),
            (syn::BinOp::Div(_), WasmType::F32) => self.emit(Instruction::F32Div),
            (syn::BinOp::Div(_), WasmType::F64) => self.emit(Instruction::F64Div),

            (syn::BinOp::Rem(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32RemS),
            (syn::BinOp::Rem(_), WasmType::I64) => self.emit(Instruction::I64RemS),
            (syn::BinOp::Rem(_), WasmType::F32 | WasmType::F64) => {
                return Err(CodegenError::unsupported(
                    "modulo on float types",
                    self.current_input_line,
                    self.current_input_column,
                ));
            }

            (syn::BinOp::And(_), _) => self.emit(Instruction::I32And),
            (syn::BinOp::Or(_), _) => self.emit(Instruction::I32Or),

            (syn::BinOp::BitXor(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Xor),
            (syn::BinOp::BitXor(_), WasmType::I64) => self.emit(Instruction::I64Xor),

            (syn::BinOp::BitAnd(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32And),
            (syn::BinOp::BitAnd(_), WasmType::I64) => self.emit(Instruction::I64And),

            (syn::BinOp::BitOr(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Or),
            (syn::BinOp::BitOr(_), WasmType::I64) => self.emit(Instruction::I64Or),

            (syn::BinOp::Shl(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Shl),
            (syn::BinOp::Shl(_), WasmType::I64) => self.emit(Instruction::I64Shl),

            (syn::BinOp::Shr(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32ShrS),
            (syn::BinOp::Shr(_), WasmType::I64) => self.emit(Instruction::I64ShrS),

            (syn::BinOp::Eq(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Eq),
            (syn::BinOp::Eq(_), WasmType::I64) => self.emit(Instruction::I64Eq),
            (syn::BinOp::Eq(_), WasmType::F32) => self.emit(Instruction::F32Eq),
            (syn::BinOp::Eq(_), WasmType::F64) => self.emit(Instruction::F64Eq),

            (syn::BinOp::Ne(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32Ne),
            (syn::BinOp::Ne(_), WasmType::I64) => self.emit(Instruction::I64Ne),
            (syn::BinOp::Ne(_), WasmType::F32) => self.emit(Instruction::F32Ne),
            (syn::BinOp::Ne(_), WasmType::F64) => self.emit(Instruction::F64Ne),

            (syn::BinOp::Lt(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32LtS),
            (syn::BinOp::Lt(_), WasmType::I64) => self.emit(Instruction::I64LtS),
            (syn::BinOp::Lt(_), WasmType::F32) => self.emit(Instruction::F32Lt),
            (syn::BinOp::Lt(_), WasmType::F64) => self.emit(Instruction::F64Lt),

            (syn::BinOp::Le(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32LeS),
            (syn::BinOp::Le(_), WasmType::I64) => self.emit(Instruction::I64LeS),
            (syn::BinOp::Le(_), WasmType::F32) => self.emit(Instruction::F32Le),
            (syn::BinOp::Le(_), WasmType::F64) => self.emit(Instruction::F64Le),

            (syn::BinOp::Gt(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32GtS),
            (syn::BinOp::Gt(_), WasmType::I64) => self.emit(Instruction::I64GtS),
            (syn::BinOp::Gt(_), WasmType::F32) => self.emit(Instruction::F32Gt),
            (syn::BinOp::Gt(_), WasmType::F64) => self.emit(Instruction::F64Gt),

            (syn::BinOp::Ge(_), WasmType::I32 | WasmType::Ptr) => self.emit(Instruction::I32GeS),
            (syn::BinOp::Ge(_), WasmType::I64) => self.emit(Instruction::I64GeS),
            (syn::BinOp::Ge(_), WasmType::F32) => self.emit(Instruction::F32Ge),
            (syn::BinOp::Ge(_), WasmType::F64) => self.emit(Instruction::F64Ge),

            _ => {
                let op_str = binop_to_string(&binary.op);
                let type_str = match ty {
                    WasmType::I32 => "i32",
                    WasmType::I64 => "i64",
                    WasmType::F32 => "f32",
                    WasmType::F64 => "f64",
                    WasmType::Ptr => "ptr",
                    WasmType::Void => "void",
                };
                return Err(CodegenError::unsupported(
                    format!(
                        "unsupported binary operator '{}' for type '{}'",
                        op_str, type_str
                    ),
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        }

        Ok(())
    }

    /// Compile a unary expression.
    fn compile_unary(&mut self, unary: &syn::ExprUnary) -> Result<(), CodegenError> {
        let ty = infer_expr_type(&unary.expr, &self.type_ctx);

        match &unary.op {
            syn::UnOp::Neg(_) => {
                // -x = 0 - x for integers, neg for floats
                match ty {
                    WasmType::I32 | WasmType::Ptr => {
                        self.emit(Instruction::I32Const(0));
                        self.compile_expr(&unary.expr)?;
                        self.emit(Instruction::I32Sub);
                    }
                    WasmType::I64 => {
                        self.emit(Instruction::I64Const(0));
                        self.compile_expr(&unary.expr)?;
                        self.emit(Instruction::I64Sub);
                    }
                    WasmType::F32 => {
                        self.compile_expr(&unary.expr)?;
                        self.emit(Instruction::F32Neg);
                    }
                    WasmType::F64 => {
                        self.compile_expr(&unary.expr)?;
                        self.emit(Instruction::F64Neg);
                    }
                    WasmType::Void => {}
                }
            }
            syn::UnOp::Not(_) => {
                self.compile_expr(&unary.expr)?;
                self.emit(Instruction::I32Eqz);
            }
            syn::UnOp::Deref(_) => {
                // Dereference - load from memory
                self.compile_expr(&unary.expr)?;
                match ty {
                    WasmType::I32 | WasmType::Ptr => {
                        self.emit(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    }
                    WasmType::I64 => {
                        self.emit(Instruction::I64Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                    }
                    WasmType::F32 => {
                        self.emit(Instruction::F32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    }
                    WasmType::F64 => {
                        self.emit(Instruction::F64Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                    }
                    WasmType::Void => {}
                }
            }
            _ => {
                let op_str = unop_to_string(&unary.op);
                return Err(CodegenError::unsupported(
                    format!("unsupported unary operator '{}'", op_str),
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        }

        Ok(())
    }

    /// Compile a function call.
    fn compile_call(&mut self, call: &syn::ExprCall) -> Result<(), CodegenError> {
        // Get function name
        let func_name = if let syn::Expr::Path(path) = call.func.as_ref() {
            path.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::")
        } else {
            return Err(CodegenError::unsupported(
                "complex function call expression",
                self.current_input_line,
                self.current_input_column,
            ));
        };

        // Check if function is supported
        let simple_name = func_name
            .split("::")
            .last()
            .unwrap_or(&func_name)
            .to_string();
        if !SUPPORTED_FUNCTIONS.contains(&simple_name.as_str())
            && self.type_ctx.get_function(&simple_name).is_none()
        {
            let suggestion = suggest_alternative(&simple_name);
            return Err(WasmError::UnsupportedFunction {
                name: func_name,
                line: self.current_input_line,
                column: self.current_input_column,
                suggestion,
            }
            .into());
        }

        // Compile arguments
        for arg in &call.args {
            self.compile_expr(arg)?;
        }

        // Find function index
        // Index 0 is the imported print function
        // User functions start at index 1
        if simple_name == "print" || simple_name == "println" {
            self.emit(Instruction::Call(0)); // print is at index 0
        } else {
            // Find the function index by counting through defined functions
            // This is a simplified approach - we'd need to track function indices properly
            self.emit(Instruction::Call(1)); // placeholder - would need proper lookup
        }

        Ok(())
    }

    /// Compile a macro (like println!).
    fn compile_macro(&mut self, mac: &syn::Macro) -> Result<(), CodegenError> {
        let macro_name = mac
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");

        match macro_name.as_str() {
            "println" | "print" => self.compile_println_macro(mac),
            _ => Err(CodegenError::unsupported(
                format!("unsupported macro: {}", macro_name),
                self.current_input_line,
                self.current_input_column,
            )),
        }
    }

    /// Compile a macro statement.
    fn compile_macro_stmt(&mut self, mac: &syn::Macro) -> Result<(), CodegenError> {
        self.compile_macro(mac)
    }

    /// Compile println! or print! macro.
    fn compile_println_macro(&mut self, mac: &syn::Macro) -> Result<(), CodegenError> {
        // Parse the macro tokens to extract the format string
        let tokens = mac.tokens.to_string();

        // Simple parsing: extract string literal
        if let Some(start) = tokens.find('"') {
            if let Some(end) = tokens[start + 1..].find('"') {
                let string_content = &tokens[start + 1..start + 1 + end];
                let source_context = format!("println!(\"{}\")", string_content);

                // Store the string
                let (offset, len) = self.store_string(
                    string_content,
                    Some(self.current_input_line),
                    Some(source_context),
                );

                // Generate print call: push ptr, push len, call $print
                self.emit(Instruction::I32Const(offset as i32));
                self.emit(Instruction::I32Const(len as i32));
                self.emit(Instruction::Call(0)); // print is import index 0

                self.add_mapping(Some("println".to_string()));
            }
        }

        Ok(())
    }

    /// Compile an if expression.
    fn compile_if(&mut self, if_expr: &syn::ExprIf) -> Result<(), CodegenError> {
        // Compile condition
        self.compile_expr(&if_expr.cond)?;

        // If-then-else structure
        self.emit(Instruction::If(wasm_encoder::BlockType::Empty));
        self.block_depth += 1;

        // Then branch
        self.compile_block(&if_expr.then_branch)?;

        // Else branch
        if let Some((_, else_branch)) = &if_expr.else_branch {
            self.emit(Instruction::Else);
            match else_branch.as_ref() {
                syn::Expr::Block(block) => {
                    self.compile_block(&block.block)?;
                }
                syn::Expr::If(else_if) => {
                    self.compile_if(else_if)?;
                }
                _ => {
                    self.compile_expr(else_branch)?;
                }
            }
        }

        self.block_depth -= 1;
        self.emit(Instruction::End);

        Ok(())
    }

    /// Compile a while loop.
    fn compile_while(&mut self, while_expr: &syn::ExprWhile) -> Result<(), CodegenError> {
        // Structure: block { loop { br_if (not cond) to block; body; br to loop } }
        self.emit(Instruction::Block(wasm_encoder::BlockType::Empty));
        self.block_depth += 1;
        self.emit(Instruction::Loop(wasm_encoder::BlockType::Empty));
        self.block_depth += 1;

        // Push loop onto stack with the block depth at the start of the loop body
        // break needs to go to depth-1 (the outer block), continue to depth (the loop)
        self.loop_stack.push((None, self.block_depth));

        // Check condition, break if false
        self.compile_expr(&while_expr.cond)?;
        self.emit(Instruction::I32Eqz);
        self.emit(Instruction::BrIf(1)); // break to outer block

        // Body
        self.compile_block(&while_expr.body)?;

        // Loop back
        self.emit(Instruction::Br(0)); // continue to loop

        self.loop_stack.pop();
        self.block_depth -= 2;

        self.emit(Instruction::End); // end loop
        self.emit(Instruction::End); // end block

        Ok(())
    }

    /// Compile an infinite loop.
    fn compile_loop(&mut self, loop_expr: &syn::ExprLoop) -> Result<(), CodegenError> {
        let label = loop_expr.label.as_ref().map(|l| l.name.ident.to_string());

        self.emit(Instruction::Block(wasm_encoder::BlockType::Empty));
        self.block_depth += 1;
        self.emit(Instruction::Loop(wasm_encoder::BlockType::Empty));
        self.block_depth += 1;

        // Push loop onto stack
        self.loop_stack.push((label, self.block_depth));

        // Body
        self.compile_block(&loop_expr.body)?;

        // Loop back
        self.emit(Instruction::Br(0)); // continue to loop

        self.loop_stack.pop();
        self.block_depth -= 2;

        self.emit(Instruction::End); // end loop
        self.emit(Instruction::End); // end block

        Ok(())
    }

    /// Compile a return statement.
    fn compile_return(&mut self, ret: &syn::ExprReturn) -> Result<(), CodegenError> {
        if let Some(expr) = &ret.expr {
            self.compile_expr(expr)?;
        }

        self.emit(Instruction::Return);

        Ok(())
    }

    /// Compile a break statement.
    fn compile_break(&mut self, _brk: &syn::ExprBreak) -> Result<(), CodegenError> {
        // Find the innermost loop and calculate relative depth to its outer block
        if let Some(&(_, loop_depth)) = self.loop_stack.last() {
            // Current depth - loop_depth gives us depth from loop start
            // +1 to get to the outer block (for break)
            let relative_depth = self.block_depth - loop_depth + 1;
            self.emit(Instruction::Br(relative_depth));
        } else {
            // No loop context - emit error or break to nearest block
            self.emit(Instruction::Br(0));
        }
        Ok(())
    }

    /// Compile a continue statement.
    fn compile_continue(&mut self, _cont: &syn::ExprContinue) -> Result<(), CodegenError> {
        // Find the innermost loop and calculate relative depth to the loop
        if let Some(&(_, loop_depth)) = self.loop_stack.last() {
            // Current depth - loop_depth gives us depth from loop start
            let relative_depth = self.block_depth - loop_depth;
            self.emit(Instruction::Br(relative_depth));
        } else {
            // No loop context
            self.emit(Instruction::Br(0));
        }
        Ok(())
    }

    /// Compile an assignment.
    fn compile_assign(&mut self, assign: &syn::ExprAssign) -> Result<(), CodegenError> {
        // Get the target variable name
        let name = if let syn::Expr::Path(path) = assign.left.as_ref() {
            if let Some(ident) = path.path.get_ident() {
                ident.to_string()
            } else {
                return Err(CodegenError::unsupported(
                    "complex assignment target",
                    self.current_input_line,
                    self.current_input_column,
                ));
            }
        } else {
            return Err(CodegenError::unsupported(
                "complex assignment target",
                self.current_input_line,
                self.current_input_column,
            ));
        };

        // Compile the value
        self.compile_expr(&assign.right)?;

        // Set the local
        if let Some(idx) = self.get_local_index(&name) {
            self.emit(Instruction::LocalSet(idx));
        } else {
            return Err(CodegenError::unsupported(
                format!("undefined variable: {}", name),
                self.current_input_line,
                self.current_input_column,
            ));
        }

        Ok(())
    }

    /// Compile a type cast.
    fn compile_cast(&mut self, cast: &syn::ExprCast) -> Result<(), CodegenError> {
        // Compile the expression
        self.compile_expr(&cast.expr)?;

        let src_ty = infer_expr_type(&cast.expr, &self.type_ctx);
        let dst_ty = if let syn::Type::Path(type_path) = cast.ty.as_ref() {
            if let Some(ident) = type_path.path.get_ident() {
                WasmType::from_rust_type(&ident.to_string())
            } else {
                WasmType::I32
            }
        } else {
            WasmType::I32
        };

        // Generate conversion instruction if needed
        if src_ty != dst_ty {
            match (src_ty, dst_ty) {
                (WasmType::I32, WasmType::I64) => self.emit(Instruction::I64ExtendI32S),
                (WasmType::I64, WasmType::I32) => self.emit(Instruction::I32WrapI64),
                (WasmType::I32, WasmType::F32) => self.emit(Instruction::F32ConvertI32S),
                (WasmType::I32, WasmType::F64) => self.emit(Instruction::F64ConvertI32S),
                (WasmType::I64, WasmType::F32) => self.emit(Instruction::F32ConvertI64S),
                (WasmType::I64, WasmType::F64) => self.emit(Instruction::F64ConvertI64S),
                (WasmType::F32, WasmType::I32) => self.emit(Instruction::I32TruncF32S),
                (WasmType::F64, WasmType::I32) => self.emit(Instruction::I32TruncF64S),
                (WasmType::F32, WasmType::I64) => self.emit(Instruction::I64TruncF32S),
                (WasmType::F64, WasmType::I64) => self.emit(Instruction::I64TruncF64S),
                (WasmType::F32, WasmType::F64) => self.emit(Instruction::F64PromoteF32),
                (WasmType::F64, WasmType::F32) => self.emit(Instruction::F32DemoteF64),
                _ => {} // No conversion needed or unsupported
            }
        }

        Ok(())
    }

    /// Compile an index expression (array access).
    fn compile_index(&mut self, index: &syn::ExprIndex) -> Result<(), CodegenError> {
        // For now, just compile as memory load
        // This is simplified - real implementation would need array bounds checking
        self.compile_expr(&index.expr)?;
        self.compile_expr(&index.index)?;
        self.emit(Instruction::I32Const(4)); // Assume 4-byte elements
        self.emit(Instruction::I32Mul);
        self.emit(Instruction::I32Add);
        self.emit(Instruction::I32Load(wasm_encoder::MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        Ok(())
    }
}

impl Default for WasmCompiler {
    fn default() -> Self {
        Self::new()
    }
}
