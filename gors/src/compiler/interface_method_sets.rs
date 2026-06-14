use super::typeinfer;
use crate::ast;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(super) struct MethodSet {
    pub(super) direct_methods: Vec<String>,
    pub(super) required_methods: Vec<String>,
    pub(super) embedded_interfaces: Vec<String>,
}

pub(super) fn for_impl(trait_name: &str, fallback_required_methods: &[String]) -> MethodSet {
    let direct_methods = direct_methods_for_impl(trait_name, fallback_required_methods);
    let required_methods = if trait_name.contains('.') {
        methods_from_import(trait_name)
            .or_else(|| super::TYPE_ENV.with(|env| env.borrow().get_interface_methods(trait_name)))
    } else {
        super::TYPE_ENV
            .with(|env| env.borrow().get_interface_methods(trait_name))
            .or_else(|| methods_from_import(trait_name))
    }
    .unwrap_or_else(|| fallback_required_methods.to_vec());
    let embedded_interfaces = embedded_interfaces_for_impl(trait_name);

    MethodSet {
        direct_methods,
        required_methods,
        embedded_interfaces,
    }
}

pub(super) fn pointer_satisfies(
    struct_method_list: &[String],
    required_methods: &[String],
) -> bool {
    required_methods
        .iter()
        .all(|method| struct_method_list.contains(method))
}

pub(super) fn pointer_type_satisfies(
    struct_name: &str,
    trait_name: &str,
    struct_method_list: &[String],
    required_methods: &[String],
) -> bool {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        if env.is_interface(trait_name) {
            let methods = satisfaction_methods(&env, trait_name, required_methods);
            env.named_type_implements_methods(struct_name, &methods, true)
        } else {
            pointer_satisfies(struct_method_list, required_methods)
        }
    })
}

pub(super) fn value_type_satisfies(
    struct_name: &str,
    trait_name: &str,
    struct_method_list: &[String],
    pointer_methods: Option<&BTreeSet<String>>,
    required_methods: &[String],
) -> bool {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        if env.is_interface(trait_name) {
            let methods = satisfaction_methods(&env, trait_name, required_methods);
            env.named_type_implements_methods(struct_name, &methods, false)
        } else {
            value_method_list_satisfies(struct_method_list, pointer_methods, required_methods)
        }
    })
}

fn satisfaction_methods(
    env: &typeinfer::TypeEnv,
    trait_name: &str,
    fallback_required_methods: &[String],
) -> Vec<String> {
    let mut methods = env.get_interface_methods(trait_name).unwrap_or_default();
    for method in fallback_required_methods {
        if !methods.contains(method) {
            methods.push(method.clone());
        }
    }
    methods
}

pub(super) fn needed_imports<'src>(decls: &[ast::Decl<'src>]) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        for decl in decls {
            collect_decl_needed_imports(decl, &env, &mut out);
        }
    });
    out
}

fn qualify_interface_param_name(
    package_name: &str,
    go_type: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
) -> Option<String> {
    match env.resolve_alias(go_type) {
        typeinfer::GoType::Interface(name) | typeinfer::GoType::Named(name) => {
            if env.is_interface(&name) {
                Some(name)
            } else if name.contains('.') {
                None
            } else {
                let qualified = format!("{package_name}.{name}");
                env.is_interface(&qualified).then_some(qualified)
            }
        }
        typeinfer::GoType::Pointer(inner) => {
            qualify_interface_param_name(package_name, &inner, env)
        }
        _ => None,
    }
}

fn collect_go_type_needed_imports(
    go_type: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match env.resolve_alias(go_type) {
        typeinfer::GoType::Interface(name) | typeinfer::GoType::Named(name) => {
            if env.is_interface(&name)
                && let Some(methods) = env.get_interface_methods(&name)
                && !methods.is_empty()
            {
                out.entry(name).or_insert(methods);
            }
        }
        typeinfer::GoType::Pointer(inner) => {
            collect_go_type_needed_imports(&inner, env, out);
        }
        _ => {}
    }
}

fn collect_named_needed_import(
    interface_name: &str,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    if env.is_interface(interface_name)
        && let Some(methods) = env.get_interface_methods(interface_name)
        && !methods.is_empty()
    {
        out.entry(interface_name.to_string()).or_insert(methods);
    }
}

fn collect_field_list_needed_imports(
    fields: Option<&ast::FieldList<'_>>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    if let Some(fields) = fields {
        for field in &fields.list {
            if let Some(type_) = &field.type_ {
                let go_type = typeinfer::GoType::from_expr(type_);
                collect_go_type_needed_imports(&go_type, env, out);
            }
        }
    }
}

fn collect_func_type_needed_imports(
    func_type: &ast::FuncType<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    collect_field_list_needed_imports(Some(&func_type.params), env, out);
    collect_field_list_needed_imports(func_type.results.as_ref(), env, out);
}

fn collect_decl_needed_imports(
    decl: &ast::Decl<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match decl {
        ast::Decl::FuncDecl(func) => {
            let collect_signature = func.recv.is_none()
                && func.name.name.starts_with("New")
                && super::reachability_context::active_roots_allow(func.name.name);
            if collect_signature {
                collect_func_type_needed_imports(&func.type_, env, out);
            }
            if let Some(body) = &func.body {
                collect_block_needed_imports(body, env, out);
            }
        }
        ast::Decl::GenDecl(gen_decl) => {
            for spec in &gen_decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_expr_needed_imports(expr, env, out);
                    }
                }
            }
        }
    }
}

fn collect_block_needed_imports(
    block: &ast::BlockStmt<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    for stmt in &block.list {
        collect_stmt_needed_imports(stmt, env, out);
    }
}

fn collect_stmt_needed_imports(
    stmt: &ast::Stmt<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match stmt {
        ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                collect_expr_needed_imports(expr, env, out);
            }
        }
        ast::Stmt::BlockStmt(block) => {
            collect_block_needed_imports(block, env, out);
        }
        ast::Stmt::CaseClause(case) => {
            if let Some(list) = &case.list {
                for expr in list {
                    collect_expr_needed_imports(expr, env, out);
                }
            }
            for stmt in &case.body {
                collect_stmt_needed_imports(stmt, env, out);
            }
        }
        ast::Stmt::CommClause(comm) => {
            if let Some(stmt) = &comm.comm {
                collect_stmt_needed_imports(stmt, env, out);
            }
            for stmt in &comm.body {
                collect_stmt_needed_imports(stmt, env, out);
            }
        }
        ast::Stmt::DeclStmt(decl) => {
            for spec in &decl.decl.specs {
                if let ast::Spec::ValueSpec(value) = spec
                    && let Some(values) = &value.values
                {
                    for expr in values {
                        collect_expr_needed_imports(expr, env, out);
                    }
                }
            }
        }
        ast::Stmt::DeferStmt(defer) => collect_call_needed_imports(&defer.call, env, out),
        ast::Stmt::ExprStmt(expr) => collect_expr_needed_imports(&expr.x, env, out),
        ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = &for_stmt.init {
                collect_stmt_needed_imports(init, env, out);
            }
            if let Some(cond) = &for_stmt.cond {
                collect_expr_needed_imports(cond, env, out);
            }
            if let Some(post) = &for_stmt.post {
                collect_stmt_needed_imports(post, env, out);
            }
            collect_block_needed_imports(&for_stmt.body, env, out);
        }
        ast::Stmt::GoStmt(go) => collect_call_needed_imports(&go.call, env, out),
        ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = &*if_stmt.init {
                collect_stmt_needed_imports(init, env, out);
            }
            collect_expr_needed_imports(&if_stmt.cond, env, out);
            collect_block_needed_imports(&if_stmt.body, env, out);
            if let Some(else_stmt) = &*if_stmt.else_ {
                collect_stmt_needed_imports(else_stmt, env, out);
            }
        }
        ast::Stmt::IncDecStmt(inc_dec) => collect_expr_needed_imports(&inc_dec.x, env, out),
        ast::Stmt::LabeledStmt(labeled) => collect_stmt_needed_imports(&labeled.stmt, env, out),
        ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                collect_expr_needed_imports(key, env, out);
            }
            if let Some(value) = &range.value {
                collect_expr_needed_imports(value, env, out);
            }
            collect_expr_needed_imports(&range.x, env, out);
            collect_block_needed_imports(&range.body, env, out);
        }
        ast::Stmt::ReturnStmt(ret) => {
            for expr in &ret.results {
                collect_expr_needed_imports(expr, env, out);
            }
        }
        ast::Stmt::SelectStmt(select) => {
            for stmt in &select.body.list {
                collect_stmt_needed_imports(stmt, env, out);
            }
        }
        ast::Stmt::SendStmt(send) => {
            collect_expr_needed_imports(&send.chan, env, out);
            collect_expr_needed_imports(&send.value, env, out);
        }
        ast::Stmt::SwitchStmt(switch) => {
            if let Some(init) = &switch.init {
                collect_stmt_needed_imports(init, env, out);
            }
            if let Some(tag) = &switch.tag {
                collect_expr_needed_imports(tag, env, out);
            }
            for stmt in &switch.body.list {
                collect_stmt_needed_imports(stmt, env, out);
            }
        }
        ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = &type_switch.init {
                collect_stmt_needed_imports(init, env, out);
            }
            collect_stmt_needed_imports(&type_switch.assign, env, out);
            for stmt in &type_switch.body.list {
                collect_stmt_needed_imports(stmt, env, out);
            }
        }
        ast::Stmt::BranchStmt(_) | ast::Stmt::EmptyStmt(_) => {}
    }
}

fn collect_expr_needed_imports(
    expr: &ast::Expr<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    match expr {
        ast::Expr::ArrayType(array) => {
            if let Some(len) = &array.len {
                collect_expr_needed_imports(len, env, out);
            }
            collect_expr_needed_imports(&array.elt, env, out);
        }
        ast::Expr::BinaryExpr(binary) => {
            collect_expr_needed_imports(&binary.x, env, out);
            collect_expr_needed_imports(&binary.y, env, out);
        }
        ast::Expr::CallExpr(call) => collect_call_needed_imports(call, env, out),
        ast::Expr::ChanType(chan) => collect_expr_needed_imports(&chan.value, env, out),
        ast::Expr::CompositeLit(lit) => {
            if let Some(type_) = &lit.type_ {
                collect_expr_needed_imports(type_, env, out);
            }
            if let Some(elts) = &lit.elts {
                for elt in elts {
                    collect_expr_needed_imports(elt, env, out);
                }
            }
        }
        ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = &ellipsis.elt {
                collect_expr_needed_imports(elt, env, out);
            }
        }
        ast::Expr::FuncLit(func) => collect_block_needed_imports(&func.body, env, out),
        ast::Expr::IndexExpr(index) => {
            collect_expr_needed_imports(&index.x, env, out);
            collect_expr_needed_imports(&index.index, env, out);
        }
        ast::Expr::IndexListExpr(index) => {
            collect_expr_needed_imports(&index.x, env, out);
            for expr in &index.indices {
                collect_expr_needed_imports(expr, env, out);
            }
        }
        ast::Expr::InterfaceType(interface) => {
            if let Some(methods) = &interface.methods {
                for field in &methods.list {
                    if let Some(type_) = &field.type_ {
                        collect_expr_needed_imports(type_, env, out);
                    }
                }
            }
        }
        ast::Expr::KeyValueExpr(key_value) => {
            collect_expr_needed_imports(&key_value.key, env, out);
            collect_expr_needed_imports(&key_value.value, env, out);
        }
        ast::Expr::MapType(map) => {
            collect_expr_needed_imports(&map.key, env, out);
            collect_expr_needed_imports(&map.value, env, out);
        }
        ast::Expr::ParenExpr(paren) => collect_expr_needed_imports(&paren.x, env, out),
        ast::Expr::SelectorExpr(selector) => collect_expr_needed_imports(&selector.x, env, out),
        ast::Expr::SliceExpr(slice) => {
            collect_expr_needed_imports(&slice.x, env, out);
            if let Some(low) = &slice.low {
                collect_expr_needed_imports(low, env, out);
            }
            if let Some(high) = &slice.high {
                collect_expr_needed_imports(high, env, out);
            }
            if let Some(max) = &slice.max {
                collect_expr_needed_imports(max, env, out);
            }
        }
        ast::Expr::StarExpr(star) => collect_expr_needed_imports(&star.x, env, out),
        ast::Expr::StructType(struct_type) => {
            if let Some(fields) = &struct_type.fields {
                for field in &fields.list {
                    if let Some(type_) = &field.type_ {
                        collect_expr_needed_imports(type_, env, out);
                    }
                }
            }
        }
        ast::Expr::TypeAssertExpr(assert) => {
            collect_expr_needed_imports(&assert.x, env, out);
            if let Some(type_) = &assert.type_ {
                if let Some(interface_name) = super::interface_name_from_type_expr(type_) {
                    collect_named_needed_import(&interface_name, env, out);
                }
                collect_expr_needed_imports(type_, env, out);
            }
        }
        ast::Expr::UnaryExpr(unary) => collect_expr_needed_imports(&unary.x, env, out),
        ast::Expr::BasicLit(_) | ast::Expr::FuncType(_) | ast::Expr::Ident(_) => {}
    }
}

fn collect_call_needed_imports(
    call: &ast::CallExpr<'_>,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    if let ast::Expr::SelectorExpr(selector) = &*call.fun
        && let ast::Expr::Ident(package) = &*selector.x
    {
        let function_name = format!("{}.{}", package.name, selector.sel.name);
        for param in env.get_func_params(&function_name) {
            collect_interface_param_needed_import(package.name, &param, env, out);
        }
        for result in env.get_func_returns(&function_name) {
            collect_interface_param_needed_import(package.name, &result, env, out);
        }
        for interface_name in env.get_func_interface_assertions(&function_name) {
            collect_named_needed_import(&interface_name, env, out);
        }
    }
    collect_expr_needed_imports(&call.fun, env, out);
    if let Some(args) = &call.args {
        for arg in args {
            collect_expr_needed_imports(arg, env, out);
        }
    }
}

fn collect_interface_param_needed_import(
    package_name: &str,
    go_type: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
    out: &mut BTreeMap<String, Vec<String>>,
) {
    let Some(interface_name) = qualify_interface_param_name(package_name, go_type, env) else {
        return;
    };
    let Some(methods) = env.get_interface_methods(&interface_name) else {
        return;
    };
    if !methods.is_empty() {
        out.entry(interface_name).or_insert(methods);
    }
}

fn direct_methods_for_impl(trait_name: &str, fallback: &[String]) -> Vec<String> {
    super::TYPE_ENV.with(|env| {
        env.borrow()
            .get_interface_direct_methods(trait_name)
            .or_else(|| direct_methods_from_import(trait_name))
            .unwrap_or_else(|| fallback.to_vec())
    })
}

fn direct_methods_from_import(trait_name: &str) -> Option<Vec<String>> {
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    env.get_interface_direct_methods(type_name)
}

fn methods_from_import(trait_name: &str) -> Option<Vec<String>> {
    let mut visiting = BTreeSet::new();
    methods_from_import_inner(trait_name, &mut visiting)
}

fn methods_from_import_inner(
    trait_name: &str,
    visiting: &mut BTreeSet<String>,
) -> Option<Vec<String>> {
    if !visiting.insert(trait_name.to_string()) {
        return Some(Vec::new());
    }
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    let mut methods = env.get_interface_direct_methods(type_name)?;
    for embedded_name in env.get_interface_direct_embedded_interfaces(type_name) {
        let embedded_name = qualify_import_interface_name(package_name, &embedded_name);
        let Some(embedded_methods) = methods_from_import_inner(&embedded_name, visiting) else {
            continue;
        };
        for method in embedded_methods {
            if !methods.contains(&method) {
                methods.push(method);
            }
        }
    }
    visiting.remove(trait_name);
    Some(methods)
}

fn embedded_interfaces_for_impl(trait_name: &str) -> Vec<String> {
    if trait_name.contains('.') {
        let imported = embedded_interfaces_from_import(trait_name).unwrap_or_default();
        if !imported.is_empty() {
            return imported;
        }
    }
    let embedded =
        super::TYPE_ENV.with(|env| env.borrow().get_interface_embedded_interfaces(trait_name));
    if !embedded.is_empty() {
        return embedded;
    }
    embedded_interfaces_from_import(trait_name).unwrap_or_default()
}

fn embedded_interfaces_from_import(trait_name: &str) -> Option<Vec<String>> {
    let mut visiting = BTreeSet::new();
    let mut out = Vec::new();
    collect_embedded_interfaces_from_import(trait_name, &mut visiting, &mut out)?;
    Some(out)
}

fn collect_embedded_interfaces_from_import(
    trait_name: &str,
    visiting: &mut BTreeSet<String>,
    out: &mut Vec<String>,
) -> Option<()> {
    if !visiting.insert(trait_name.to_string()) {
        return Some(());
    }
    let (package_name, type_name) = trait_name.split_once('.')?;
    let (_, env) = crate::resolve::scan_type_env(package_name)?;
    for embedded_name in env.get_interface_direct_embedded_interfaces(type_name) {
        let embedded_name = qualify_import_interface_name(package_name, &embedded_name);
        if !out.contains(&embedded_name) {
            out.push(embedded_name.clone());
        }
        collect_embedded_interfaces_from_import(&embedded_name, visiting, out)?;
    }
    visiting.remove(trait_name);
    Some(())
}

fn qualify_import_interface_name(package_name: &str, embedded_name: &str) -> String {
    if embedded_name.contains('.') {
        embedded_name.to_string()
    } else {
        format!("{package_name}.{embedded_name}")
    }
}

pub(super) fn value_method_list_satisfies(
    struct_method_list: &[String],
    pointer_methods: Option<&BTreeSet<String>>,
    required_methods: &[String],
) -> bool {
    required_methods.iter().all(|method| {
        struct_method_list.contains(method)
            && pointer_methods.is_none_or(|methods| !methods.contains(method))
    })
}
