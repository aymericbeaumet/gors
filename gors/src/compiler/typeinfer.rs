use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::token;

#[derive(Debug, Clone, PartialEq)]
pub enum GoType {
    Bool,
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uintptr,
    Float32,
    Float64,
    Complex64,
    Complex128,
    String,
    Slice(Box<GoType>),
    Map(Box<GoType>, Box<GoType>),
    Pointer(Box<GoType>),
    Array(Box<GoType>),
    Chan(Box<GoType>),
    Func {
        params: Vec<GoType>,
        results: Vec<GoType>,
    },
    Named(std::string::String),
    Interface(std::string::String),
    Any,
    Error,
    Unknown,
}

impl GoType {
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            GoType::Int
                | GoType::Int8
                | GoType::Int16
                | GoType::Int32
                | GoType::Int64
                | GoType::Uint
                | GoType::Uint8
                | GoType::Uint16
                | GoType::Uint32
                | GoType::Uint64
                | GoType::Uintptr
        )
    }

    pub fn is_signed_int(&self) -> bool {
        matches!(
            self,
            GoType::Int | GoType::Int8 | GoType::Int16 | GoType::Int32 | GoType::Int64
        )
    }

    pub fn is_unsigned_int(&self) -> bool {
        matches!(
            self,
            GoType::Uint
                | GoType::Uint8
                | GoType::Uint16
                | GoType::Uint32
                | GoType::Uint64
                | GoType::Uintptr
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, GoType::Float32 | GoType::Float64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_string(&self) -> bool {
        matches!(self, GoType::String)
    }

    pub fn is_interface(&self) -> bool {
        matches!(self, GoType::Any | GoType::Error | GoType::Interface(_))
    }

    /// Returns the Rust type that len() returns for this type.
    pub fn len_type(&self) -> GoType {
        GoType::Int
    }

    pub fn from_expr(expr: &ast::Expr) -> GoType {
        match expr {
            ast::Expr::Ident(id) => GoType::from_name(id.name),
            ast::Expr::StarExpr(star) => GoType::Pointer(Box::new(GoType::from_expr(&star.x))),
            ast::Expr::ArrayType(arr) => {
                let elem = arr.elt.as_ref();
                let elem_type = GoType::from_expr(elem);
                if arr.len.is_some() {
                    GoType::Array(Box::new(elem_type))
                } else {
                    GoType::Slice(Box::new(elem_type))
                }
            }
            ast::Expr::MapType(map) => GoType::Map(
                Box::new(GoType::from_expr(&map.key)),
                Box::new(GoType::from_expr(&map.value)),
            ),
            ast::Expr::ChanType(chan) => GoType::Chan(Box::new(GoType::from_expr(&chan.value))),
            ast::Expr::InterfaceType(_) => GoType::Any,
            ast::Expr::Ellipsis(e) => {
                if let Some(elt) = &e.elt {
                    GoType::Slice(Box::new(GoType::from_expr(elt)))
                } else {
                    GoType::Slice(Box::new(GoType::Any))
                }
            }
            ast::Expr::SelectorExpr(sel) => {
                if let ast::Expr::Ident(pkg) = &*sel.x {
                    GoType::Named(format!("{}.{}", pkg.name, sel.sel.name))
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::FuncType(ft) => {
                let params: Vec<GoType> = ft
                    .params
                    .list
                    .iter()
                    .flat_map(|f| {
                        let ty = f
                            .type_
                            .as_ref()
                            .map(GoType::from_expr)
                            .unwrap_or(GoType::Unknown);
                        let count = f.names.as_ref().map_or(1, |n| n.len());
                        std::iter::repeat(ty).take(count)
                    })
                    .collect();
                let results: Vec<GoType> = ft
                    .results
                    .as_ref()
                    .map(|r| {
                        r.list
                            .iter()
                            .flat_map(|f| {
                                let ty = f
                                    .type_
                                    .as_ref()
                                    .map(GoType::from_expr)
                                    .unwrap_or(GoType::Unknown);
                                let count = f.names.as_ref().map_or(1, |n| n.len());
                                std::iter::repeat(ty).take(count)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                GoType::Func { params, results }
            }
            _ => GoType::Unknown,
        }
    }

    pub fn from_name(name: &str) -> GoType {
        match name {
            "bool" => GoType::Bool,
            "int" => GoType::Int,
            "int8" => GoType::Int8,
            "int16" => GoType::Int16,
            "int32" | "rune" => GoType::Int32,
            "int64" => GoType::Int64,
            "uint" => GoType::Uint,
            "uint8" | "byte" => GoType::Uint8,
            "uint16" => GoType::Uint16,
            "uint32" => GoType::Uint32,
            "uint64" => GoType::Uint64,
            "uintptr" => GoType::Uintptr,
            "float32" => GoType::Float32,
            "float64" => GoType::Float64,
            "complex64" => GoType::Complex64,
            "complex128" => GoType::Complex128,
            "string" => GoType::String,
            "error" => GoType::Error,
            "any" => GoType::Any,
            _ => GoType::Named(name.to_string()),
        }
    }

    /// Infer the type of a Go expression given the current type environment.
    pub fn infer_expr(expr: &ast::Expr, env: &TypeEnv) -> GoType {
        match expr {
            ast::Expr::BasicLit(lit) => match lit.kind {
                token::Token::INT => GoType::Int,
                token::Token::FLOAT => GoType::Float64,
                token::Token::STRING => GoType::String,
                token::Token::CHAR => GoType::Int32,
                _ => GoType::Unknown,
            },
            ast::Expr::Ident(id) => match id.name {
                "true" | "false" => GoType::Bool,
                "nil" => GoType::Any,
                name => env.get_var(name).unwrap_or(GoType::Unknown),
            },
            ast::Expr::UnaryExpr(u) => GoType::infer_expr(&u.x, env),
            ast::Expr::BinaryExpr(bin) => {
                match bin.op {
                    // Comparison operators produce bool
                    token::Token::EQL
                    | token::Token::NEQ
                    | token::Token::LSS
                    | token::Token::GTR
                    | token::Token::LEQ
                    | token::Token::GEQ
                    | token::Token::LAND
                    | token::Token::LOR => GoType::Bool,
                    // Arithmetic preserves the type of the left operand
                    _ => GoType::infer_expr(&bin.x, env),
                }
            }
            ast::Expr::CallExpr(call) => {
                // For function calls, return the first result type
                match &*call.fun {
                    ast::Expr::Ident(id) => {
                        // Builtin functions
                        match id.name {
                            "len" | "cap" => GoType::Int,
                            "make" => {
                                // infer from first arg which is the type
                                call.args
                                    .as_ref()
                                    .and_then(|a| a.first())
                                    .map(|e| GoType::from_expr(e))
                                    .unwrap_or(GoType::Unknown)
                            }
                            "new" => {
                                let inner = call
                                    .args
                                    .as_ref()
                                    .and_then(|a| a.first())
                                    .map(|e| GoType::from_expr(e))
                                    .unwrap_or(GoType::Unknown);
                                GoType::Pointer(Box::new(inner))
                            }
                            "append" => call
                                .args
                                .as_ref()
                                .and_then(|a| a.first())
                                .map(|e| GoType::infer_expr(e, env))
                                .unwrap_or(GoType::Unknown),
                            "string" => GoType::String,
                            "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8"
                            | "uint16" | "uint32" | "uint64" | "uintptr" | "float32"
                            | "float64" | "byte" | "rune" | "bool" => GoType::from_name(id.name),
                            _ => env.get_func_return(id.name),
                        }
                    }
                    ast::Expr::SelectorExpr(sel) => {
                        if let ast::Expr::Ident(pkg) = &*sel.x {
                            let key = format!("{}.{}", pkg.name, sel.sel.name);
                            env.get_func_return(&key)
                        } else {
                            GoType::Unknown
                        }
                    }
                    _ => GoType::Unknown,
                }
            }
            ast::Expr::IndexExpr(_) => {
                // Indexing: the result type depends on the container type
                // For now, return Unknown (could be refined)
                GoType::Unknown
            }
            ast::Expr::SelectorExpr(sel) => {
                // Field access: would need struct type info
                // For reflect.Value method calls, we know the return types
                if let ast::Expr::Ident(id) = &*sel.x {
                    let var_type = env.get_var(id.name);
                    match var_type {
                        Some(GoType::Named(ref name)) => env.get_field_type(name, sel.sel.name),
                        Some(GoType::Pointer(inner)) => {
                            if let GoType::Named(ref name) = *inner {
                                env.get_field_type(name, sel.sel.name)
                            } else {
                                GoType::Unknown
                            }
                        }
                        _ => GoType::Unknown,
                    }
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::TypeAssertExpr(ta) => {
                if let Some(type_expr) = &ta.type_ {
                    GoType::from_expr(type_expr)
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::CompositeLit(cl) => {
                if let Some(type_expr) = &cl.type_ {
                    GoType::from_expr(type_expr)
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::SliceExpr(slice) => {
                let base = GoType::infer_expr(&slice.x, env);
                match env.resolve_alias(&base) {
                    GoType::String => GoType::String,
                    GoType::Slice(elem) => GoType::Slice(elem),
                    GoType::Array(elem) => GoType::Slice(elem),
                    GoType::Named(name) => GoType::Named(name),
                    other => other,
                }
            }
            ast::Expr::StarExpr(star) => {
                let inner = GoType::infer_expr(&star.x, env);
                if let GoType::Pointer(inner_type) = inner {
                    *inner_type
                } else {
                    GoType::Unknown
                }
            }
            ast::Expr::ParenExpr(p) => GoType::infer_expr(&p.x, env),
            _ => GoType::Unknown,
        }
    }
}

/// Type environment for tracking Go types during compilation.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Variable name → Go type (current scope)
    vars: HashMap<std::string::String, GoType>,
    /// Function/method name → return types
    funcs: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → parameter types
    func_params: HashMap<std::string::String, Vec<GoType>>,
    /// Function/method name → index where a variadic parameter starts
    func_variadic_start: HashMap<std::string::String, usize>,
    /// Type name → kind (struct, interface, alias)
    type_kinds: HashMap<std::string::String, TypeKind>,
    /// Struct name → field types
    struct_fields: HashMap<std::string::String, Vec<(std::string::String, GoType)>>,
    /// Package-level string constants emitted as owned-String functions.
    string_consts: HashSet<std::string::String>,
    consts: HashSet<std::string::String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Interface,
    Alias(GoType),
}

impl TypeEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_var(&mut self, name: &str, ty: GoType) {
        self.vars.insert(name.to_string(), ty);
    }

    pub fn get_var(&self, name: &str) -> Option<GoType> {
        self.vars.get(name).cloned()
    }

    pub fn set_func(&mut self, name: &str, returns: Vec<GoType>) {
        self.funcs.insert(name.to_string(), returns);
    }

    pub fn set_func_params(&mut self, name: &str, params: Vec<GoType>) {
        self.func_params.insert(name.to_string(), params);
    }

    pub fn set_func_variadic_start(&mut self, name: &str, start: usize) {
        self.func_variadic_start.insert(name.to_string(), start);
    }

    pub fn get_func_variadic_start(&self, name: &str) -> Option<usize> {
        self.func_variadic_start.get(name).copied()
    }

    pub fn get_func_params(&self, name: &str) -> Vec<GoType> {
        self.func_params.get(name).cloned().unwrap_or_default()
    }

    pub fn get_func_return(&self, name: &str) -> GoType {
        self.funcs
            .get(name)
            .and_then(|r| r.first().cloned())
            .unwrap_or(GoType::Unknown)
    }

    pub fn get_func_returns(&self, name: &str) -> Vec<GoType> {
        self.funcs.get(name).cloned().unwrap_or_default()
    }

    pub fn set_type_kind(&mut self, name: &str, kind: TypeKind) {
        self.type_kinds.insert(name.to_string(), kind);
    }

    pub fn get_type_kind(&self, name: &str) -> Option<&TypeKind> {
        self.type_kinds.get(name)
    }

    pub fn is_interface(&self, name: &str) -> bool {
        matches!(self.type_kinds.get(name), Some(TypeKind::Interface))
    }

    pub fn resolve_alias(&self, ty: &GoType) -> GoType {
        match ty {
            GoType::Named(name) => match self.type_kinds.get(name) {
                Some(TypeKind::Alias(inner)) => self.resolve_alias(inner),
                _ => ty.clone(),
            },
            GoType::Pointer(inner) => GoType::Pointer(Box::new(self.resolve_alias(inner))),
            _ => ty.clone(),
        }
    }

    pub fn set_struct_fields(&mut self, name: &str, fields: Vec<(std::string::String, GoType)>) {
        self.struct_fields.insert(name.to_string(), fields);
    }

    pub fn get_field_type(&self, struct_name: &str, field_name: &str) -> GoType {
        self.struct_fields
            .get(struct_name)
            .and_then(|fields| {
                fields
                    .iter()
                    .find(|(n, _)| n == field_name)
                    .map(|(_, t)| t.clone())
            })
            .unwrap_or(GoType::Unknown)
    }

    pub fn get_struct_fields(&self, struct_name: &str) -> Vec<(std::string::String, GoType)> {
        self.struct_fields
            .get(struct_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_string_const(&mut self, name: &str) {
        self.string_consts.insert(name.to_string());
    }

    pub fn set_const(&mut self, name: &str) {
        self.consts.insert(name.to_string());
    }

    pub fn is_const(&self, name: &str) -> bool {
        self.consts.contains(name)
    }

    pub fn is_string_const(&self, name: &str) -> bool {
        self.string_consts.contains(name)
    }

    pub fn string_const_names(&self) -> HashSet<std::string::String> {
        self.string_consts.clone()
    }

    pub fn merge_package(&mut self, package_name: &str, package_env: &TypeEnv) {
        for (name, returns) in &package_env.funcs {
            self.set_func(&format!("{package_name}.{name}"), returns.clone());
        }
        for (name, params) in &package_env.func_params {
            self.set_func_params(&format!("{package_name}.{name}"), params.clone());
        }
        for (name, start) in &package_env.func_variadic_start {
            self.set_func_variadic_start(&format!("{package_name}.{name}"), *start);
        }
        for (name, kind) in &package_env.type_kinds {
            self.set_type_kind(&format!("{package_name}.{name}"), kind.clone());
        }
        for (name, fields) in &package_env.struct_fields {
            self.set_struct_fields(&format!("{package_name}.{name}"), fields.clone());
        }
    }

    /// Pre-scan a Go AST file to populate type declarations and function signatures.
    pub fn scan_file(&mut self, file: &ast::File) {
        for decl in &file.decls {
            match decl {
                ast::Decl::GenDecl(gd) => {
                    for spec in &gd.specs {
                        match spec {
                            ast::Spec::TypeSpec(ts) => {
                                self.scan_type_spec(ts);
                            }
                            ast::Spec::ValueSpec(vs) => {
                                self.scan_value_spec(vs, gd.tok);
                            }
                            _ => {}
                        }
                    }
                }
                ast::Decl::FuncDecl(fd) => {
                    self.scan_func_decl(fd);
                }
            }
        }
    }

    fn scan_type_spec(&mut self, ts: &ast::TypeSpec) {
        let Some(ref name) = ts.name else { return };
        match &ts.type_ {
            ast::Expr::StructType(st) => {
                self.set_type_kind(name.name, TypeKind::Struct);
                if let Some(ref field_list) = st.fields {
                    let mut fields = vec![];
                    for field in &field_list.list {
                        let ty = field
                            .type_
                            .as_ref()
                            .map(GoType::from_expr)
                            .unwrap_or(GoType::Unknown);
                        if let Some(ref names) = field.names {
                            for n in names {
                                fields.push((n.name.to_string(), ty.clone()));
                            }
                        } else if let Some(type_expr) = &field.type_
                            && let Some(name) = embedded_field_name(type_expr)
                        {
                            fields.push((name, ty.clone()));
                        }
                    }
                    self.set_struct_fields(name.name, fields);
                }
            }
            ast::Expr::InterfaceType(_) => {
                self.set_type_kind(name.name, TypeKind::Interface);
            }
            other => {
                let underlying = GoType::from_expr(other);
                self.set_type_kind(name.name, TypeKind::Alias(underlying));
            }
        }
    }

    fn scan_value_spec(&mut self, vs: &ast::ValueSpec, tok: token::Token) {
        let explicit_type = vs.type_.as_ref().map(GoType::from_expr);
        let values = vs.values.as_ref();

        for (i, name) in vs.names.iter().enumerate() {
            if tok == token::Token::CONST {
                self.set_const(name.name);
            }
            let ty = if let Some(ref et) = explicit_type {
                et.clone()
            } else if tok == token::Token::CONST {
                // Infer from value
                values
                    .and_then(|v| v.get(i))
                    .map(|e| GoType::infer_expr(e, self))
                    .unwrap_or(GoType::Int)
            } else {
                GoType::Unknown
            };
            if tok == token::Token::CONST && matches!(ty, GoType::String) {
                self.set_string_const(name.name);
            }
            self.set_var(name.name, ty);
        }
    }

    fn scan_func_decl(&mut self, fd: &ast::FuncDecl) {
        let name = fd.name.name;

        let mut variadic_start = None;
        let mut param_count = 0;
        let params: Vec<GoType> = fd
            .type_
            .params
            .list
            .iter()
            .flat_map(|f| {
                let ty = f
                    .type_
                    .as_ref()
                    .map(GoType::from_expr)
                    .unwrap_or(GoType::Unknown);
                let count = f.names.as_ref().map_or(1, |n| n.len());
                if matches!(f.type_, Some(ast::Expr::Ellipsis(_))) {
                    variadic_start = Some(param_count);
                }
                param_count += count;
                std::iter::repeat(ty).take(count)
            })
            .collect();

        // Collect return types
        let returns: Vec<GoType> = fd
            .type_
            .results
            .as_ref()
            .map(|r| {
                r.list
                    .iter()
                    .flat_map(|f| {
                        let ty = f
                            .type_
                            .as_ref()
                            .map(GoType::from_expr)
                            .unwrap_or(GoType::Unknown);
                        let count = f.names.as_ref().map_or(1, |n| n.len());
                        std::iter::repeat(ty).take(count)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // If it's a method, register with receiver type prefix
        if let Some(ref recv) = fd.recv {
            if let Some(recv_field) = recv.list.first() {
                if let Some(ref recv_type) = recv_field.type_ {
                    let recv_name = extract_type_name(recv_type);
                    let method_key = format!("{}.{}", recv_name, name);
                    self.set_func_params(&method_key, params.clone());
                    self.set_func(&method_key, returns.clone());
                    if let Some(start) = variadic_start {
                        self.set_func_variadic_start(&method_key, start);
                    }
                }
            }
        }
        self.set_func_params(name, params);
        self.set_func(name, returns);
        if let Some(start) = variadic_start {
            self.set_func_variadic_start(name, start);
        }

        // Register parameter types
        for param in &fd.type_.params.list {
            let ty = param
                .type_
                .as_ref()
                .map(GoType::from_expr)
                .unwrap_or(GoType::Unknown);
            if let Some(ref names) = param.names {
                for n in names {
                    self.set_var(n.name, ty.clone());
                }
            }
        }

        // Register named return value types
        if let Some(ref results) = fd.type_.results {
            for field in &results.list {
                let ty = field
                    .type_
                    .as_ref()
                    .map(GoType::from_expr)
                    .unwrap_or(GoType::Unknown);
                if let Some(ref names) = field.names {
                    for n in names {
                        self.set_var(n.name, ty.clone());
                    }
                }
            }
        }
    }
}

fn embedded_field_name(expr: &ast::Expr) -> Option<String> {
    match expr {
        ast::Expr::Ident(id) => Some(id.name.to_string()),
        ast::Expr::StarExpr(star) => embedded_field_name(&star.x),
        ast::Expr::SelectorExpr(sel) => Some(sel.sel.name.to_string()),
        ast::Expr::IndexExpr(index) => embedded_field_name(&index.x),
        ast::Expr::IndexListExpr(index) => embedded_field_name(&index.x),
        _ => None,
    }
}

fn extract_type_name<'a>(expr: &'a ast::Expr<'a>) -> &'a str {
    match expr {
        ast::Expr::Ident(id) => id.name,
        ast::Expr::StarExpr(star) => extract_type_name(&star.x),
        _ => "",
    }
}
