use flate2::read::GzDecoder;
use quote::ToTokens;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};

static STDLIB_ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/go_stdlib.tar.gz"));
static STDLIB_INDEX: &str = include_str!(concat!(env!("OUT_DIR"), "/go_stdlib.index"));

type PackageFiles = Vec<(String, String)>;
type TypeEnv = crate::compiler::typeinfer::TypeEnv;

static PACKAGE_INDEX: OnceLock<Vec<&'static str>> = OnceLock::new();
static PACKAGE_FILES: OnceLock<Mutex<HashMap<String, Option<Arc<PackageFiles>>>>> = OnceLock::new();
static TYPE_ENVS: OnceLock<Mutex<HashMap<String, Option<(String, TypeEnv)>>>> = OnceLock::new();
static TRANSITIVE_IMPORTS: OnceLock<Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
static PANIC_HOOK_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

thread_local! {
    static RESOLVED_MODULES: RefCell<HashMap<String, Option<syn::ItemMod>>> = RefCell::new(HashMap::new());
}

fn package_index() -> &'static Vec<&'static str> {
    PACKAGE_INDEX.get_or_init(|| {
        let mut packages: Vec<_> = STDLIB_INDEX
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        packages.sort_unstable();
        packages.dedup();
        packages
    })
}

fn package_file_cache() -> &'static Mutex<HashMap<String, Option<Arc<PackageFiles>>>> {
    PACKAGE_FILES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    let decoder = GzDecoder::new(STDLIB_ARCHIVE);
    let mut archive = tar::Archive::new(decoder);
    let mut files: PackageFiles = Vec::new();

    let Ok(entries) = archive.entries() else {
        return None;
    };

    for entry in entries.flatten() {
        let mut entry = entry;
        let path = match entry.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        if !path.ends_with(".go") {
            continue;
        }

        let filename = match path.rsplit_once('/') {
            Some((dir, file)) if dir == import_path => file.to_string(),
            Some(_) => continue,
            None => continue,
        };

        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() && should_compile_file(&filename, &content) {
            files.push((filename, content));
        }
    }

    if files.is_empty() {
        return None;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Some(Arc::new(files))
}

fn type_envs() -> &'static Mutex<HashMap<String, Option<(String, TypeEnv)>>> {
    TYPE_ENVS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn transitive_imports() -> &'static Mutex<HashMap<String, Vec<String>>> {
    TRANSITIVE_IMPORTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_known(import_path: &str) -> bool {
    package_exists(import_path)
}

pub fn package_exists(import_path: &str) -> bool {
    package_index()
        .binary_search_by(|candidate| candidate.cmp(&import_path))
        .is_ok()
}

pub fn package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    if !package_exists(import_path) {
        return None;
    }
    if let Ok(cache) = package_file_cache().lock()
        && let Some(files) = cache.get(import_path)
    {
        return files.clone();
    }

    let files = load_package_files(import_path);
    if let Ok(mut cache) = package_file_cache().lock() {
        cache.insert(import_path.to_string(), files.clone());
    }
    files
}

pub fn list_packages() -> Vec<String> {
    package_index()
        .iter()
        .map(|package| (*package).to_string())
        .collect()
}

pub fn module_name(import_path: &str) -> String {
    let mut out = String::new();
    for ch in import_path.chars() {
        match ch {
            '/' => out.push_str("__"),
            ch if ch.is_ascii_alphanumeric() || ch == '_' => out.push(ch),
            _ => out.push('_'),
        }
    }

    if out.is_empty() {
        out.push('_');
    }

    if out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }

    if is_rust_keyword(&out) {
        out.push('_');
    }

    out
}

pub fn resolve(import_path: &str) -> Option<syn::ItemMod> {
    resolve_cached(import_path, None)
}

pub fn resolve_with_roots(import_path: &str, roots: &HashSet<String>) -> Option<syn::ItemMod> {
    if roots.is_empty() {
        return None;
    }
    resolve_cached(import_path, Some(roots))
}

fn resolve_cached(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let cache_key = resolve_cache_key(import_path, roots);
    if let Some(cached) = RESOLVED_MODULES.with(|cache| cache.borrow().get(&cache_key).cloned()) {
        return cached;
    }

    let resolved = resolve_uncached(import_path, roots);
    RESOLVED_MODULES.with(|cache| {
        cache.borrow_mut().insert(cache_key, resolved.clone());
    });
    resolved
}

fn resolve_cache_key(import_path: &str, roots: Option<&HashSet<String>>) -> String {
    let Some(roots) = roots else {
        return import_path.to_string();
    };
    let mut roots: Vec<_> = roots.iter().map(String::as_str).collect();
    roots.sort_unstable();
    format!("{import_path}\0{}", roots.join(","))
}

fn resolve_uncached(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let files = package_files(import_path)?;
    let mod_name = module_name(import_path);

    let mut parsed_files = Vec::new();
    for (filename, content) in files.iter() {
        let ast = match crate::parser::parse_file(filename, content) {
            Ok(ast) => ast,
            Err(e) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: parse error: {e}"
                ));
                continue;
            }
        };
        parsed_files.push((filename.as_str(), ast));
    }

    let reachable_names = roots.map(|roots| reachable_package_names(&parsed_files, roots));
    if roots.is_some() && reachable_names.as_ref().is_none_or(HashSet::is_empty) {
        cache_transitive_imports(import_path, Vec::new());
        return None;
    }

    let parsed_files: Vec<_> = parsed_files
        .into_iter()
        .filter_map(|(filename, ast)| {
            let filtered = match &reachable_names {
                Some(reachable) => filter_file_to_reachable(ast, reachable),
                None => ast,
            };
            file_has_compilable_decl(&filtered).then_some((filename, filtered))
        })
        .collect();

    if parsed_files.is_empty() {
        cache_transitive_imports(import_path, Vec::new());
        return None;
    }

    let mut package_type_env = TypeEnv::new();
    for (_, ast) in &parsed_files {
        package_type_env.scan_file(ast);
    }
    let mut imported_type_envs: BTreeMap<String, (String, TypeEnv)> = BTreeMap::new();
    for (_, ast) in &parsed_files {
        for import in ast.imports() {
            let imported_path = import.path.value.trim_matches('"');
            if imported_path == import_path {
                continue;
            }
            if let Some((package_name, env)) = scan_type_env(imported_path) {
                imported_type_envs.insert(imported_path.to_string(), (package_name, env));
            }
        }
    }

    let import_renames = package_import_renames(&parsed_files);
    let import_path_by_module = package_import_path_by_module(&parsed_files);
    let mut all_items: Vec<syn::Item> = Vec::new();

    for (filename, ast) in parsed_files {
        let mut type_env = package_type_env.clone();
        crate::compiler::merge_import_type_envs(
            &mut type_env,
            &ast,
            &BTreeMap::new(),
            &imported_type_envs,
        );
        let compiled = match catch_unwind_quiet(std::panic::AssertUnwindSafe(|| {
            crate::compiler::compile_with_type_env_and_import_renames(
                ast,
                type_env,
                import_renames.clone(),
            )
        })) {
            Ok(Ok(compiled)) => compiled,
            Ok(Err(e)) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: compile error: {e}"
                ));
                continue;
            }
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: panic: {msg}"
                ));
                continue;
            }
        };
        all_items.extend(compiled.items);
    }

    if all_items.is_empty() {
        cache_transitive_imports(import_path, Vec::new());
        return None;
    }

    let mut merged_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: all_items,
    };
    crate::compiler::passes::pass_after_package_merge(&mut merged_file);
    let mut all_items = merged_file.items;

    dedupe_use_items(&mut all_items);
    let used_imports = used_imports_from_items(&mut all_items, &import_path_by_module);
    cache_transitive_imports(import_path, used_imports.clone());
    let module_refs: HashSet<String> = used_imports.iter().map(|path| module_name(path)).collect();
    inject_structural_helpers(&mut all_items);
    prefix_crate_paths(&mut all_items, &module_refs);

    Some(syn::ItemMod {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        unsafety: None,
        mod_token: <syn::Token![mod]>::default(),
        ident: syn::Ident::new(&mod_name, proc_macro2::Span::mixed_site()),
        content: Some((syn::token::Brace::default(), all_items)),
        semi: None,
    })
}

fn dedupe_use_items(items: &mut Vec<syn::Item>) {
    let mut seen = HashSet::new();
    items.retain(|item| {
        let syn::Item::Use(item_use) = item else {
            return true;
        };
        seen.insert(item_use.to_token_stream().to_string())
    });
}

fn catch_unwind_quiet<F, R>(f: F) -> std::thread::Result<R>
where
    F: FnOnce() -> R + std::panic::UnwindSafe,
{
    let hook_lock = PANIC_HOOK_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = hook_lock.lock() else {
        return std::panic::catch_unwind(f);
    };

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(f);
    std::panic::set_hook(previous_hook);
    result
}

fn log_skip(args: std::fmt::Arguments<'_>) {
    if std::env::var("GORS_STDLIB_TRACE").is_ok_and(|value| value == "1" || value == "true") {
        eprintln!("{args}");
    }
}

pub fn scan_type_env(import_path: &str) -> Option<(String, TypeEnv)> {
    if let Ok(cache) = type_envs().lock() {
        if let Some(cached) = cache.get(import_path) {
            return cached.clone();
        }
    }

    let scanned = scan_type_env_uncached(import_path);
    if let Ok(mut cache) = type_envs().lock() {
        cache.insert(import_path.to_string(), scanned.clone());
    }
    scanned
}

fn scan_type_env_uncached(import_path: &str) -> Option<(String, TypeEnv)> {
    let files = package_files(import_path)?;
    let mut env = TypeEnv::new();
    let mut package_name = None;

    for (filename, content) in files.iter() {
        let Ok(ast) = crate::parser::parse_file(filename, content) else {
            continue;
        };
        if package_name.is_none() {
            package_name = Some(ast.name.name.to_string());
        }
        env.scan_file(&ast);
    }

    package_name.map(|name| (name, env))
}

pub fn collect_transitive_imports(import_path: &str) -> Vec<String> {
    if let Ok(cache) = transitive_imports().lock() {
        if let Some(cached) = cache.get(import_path) {
            return cached.clone();
        }
    }

    let imports = collect_transitive_imports_uncached(import_path);
    cache_transitive_imports(import_path, imports.clone());
    imports
}

fn cache_transitive_imports(import_path: &str, imports: Vec<String>) {
    if let Ok(mut cache) = transitive_imports().lock() {
        cache.insert(import_path.to_string(), imports);
    }
}

fn collect_transitive_imports_uncached(import_path: &str) -> Vec<String> {
    let Some(files) = package_files(import_path) else {
        return vec![];
    };

    let mut imports = HashSet::new();
    for (filename, content) in files.iter() {
        if let Ok(ast) = crate::parser::parse_file(filename, content) {
            for import in ast.imports() {
                let path = import.path.value.trim_matches('"');
                if is_known(path) && path != import_path {
                    imports.insert(path.to_string());
                }
            }
            continue;
        }

        for path in import_paths_from_source(content) {
            if is_known(&path) && path != import_path {
                imports.insert(path);
            }
        }
    }

    let mut imports: Vec<_> = imports.into_iter().collect();
    imports.sort();
    imports
}

fn reachable_package_names(
    parsed_files: &[(&str, crate::ast::File<'_>)],
    roots: &HashSet<String>,
) -> HashSet<String> {
    let top_names = package_top_level_names(parsed_files);
    let mut reachable: HashSet<String> = roots
        .iter()
        .filter(|name| top_names.contains(name.as_str()))
        .cloned()
        .collect();

    let mut changed = true;
    while changed {
        changed = false;
        for (_, file) in parsed_files {
            for decl in &file.decls {
                if !decl_is_reachable(decl, &reachable) {
                    continue;
                }
                for name in decl_names(decl) {
                    changed |= reachable.insert(name);
                }
                let mut refs = HashSet::new();
                refs_from_decl(decl, &mut refs);
                for reference in refs {
                    if top_names.contains(reference.as_str()) {
                        changed |= reachable.insert(reference);
                    }
                }
            }
        }
    }

    reachable
}

fn package_top_level_names(parsed_files: &[(&str, crate::ast::File<'_>)]) -> HashSet<String> {
    parsed_files
        .iter()
        .flat_map(|(_, file)| file.decls.iter())
        .flat_map(decl_names)
        .collect()
}

fn decl_names(decl: &crate::ast::Decl<'_>) -> Vec<String> {
    match decl {
        crate::ast::Decl::FuncDecl(func) if func.recv.is_none() => {
            vec![func.name.name.to_string()]
        }
        crate::ast::Decl::FuncDecl(_) => Vec::new(),
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .flat_map(spec_names)
            .collect::<Vec<_>>(),
    }
}

fn spec_names(spec: &crate::ast::Spec<'_>) -> Vec<String> {
    match spec {
        crate::ast::Spec::ImportSpec(_) => Vec::new(),
        crate::ast::Spec::TypeSpec(type_spec) => type_spec
            .name
            .as_ref()
            .map(|name| vec![name.name.to_string()])
            .unwrap_or_default(),
        crate::ast::Spec::ValueSpec(value_spec) => value_spec
            .names
            .iter()
            .map(|name| name.name.to_string())
            .collect(),
    }
}

fn decl_is_reachable(decl: &crate::ast::Decl<'_>, reachable: &HashSet<String>) -> bool {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            reachable.contains(func.name.name)
                || receiver_type_name(func).is_some_and(|name| reachable.contains(&name))
        }
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .any(|spec| spec_names(spec).iter().any(|name| reachable.contains(name))),
    }
}

fn receiver_type_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    func.recv
        .as_ref()
        .and_then(|recv| recv.list.first())
        .and_then(|field| field.type_.as_ref())
        .and_then(named_type_from_expr)
}

fn named_type_from_expr(expr: &crate::ast::Expr<'_>) -> Option<String> {
    match expr {
        crate::ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        crate::ast::Expr::StarExpr(star) => named_type_from_expr(&star.x),
        crate::ast::Expr::ParenExpr(paren) => named_type_from_expr(&paren.x),
        crate::ast::Expr::IndexExpr(index) => named_type_from_expr(&index.x),
        crate::ast::Expr::IndexListExpr(index) => named_type_from_expr(&index.x),
        _ => None,
    }
}

fn filter_file_to_reachable<'a>(
    mut file: crate::ast::File<'a>,
    reachable: &HashSet<String>,
) -> crate::ast::File<'a> {
    file.decls = file
        .decls
        .into_iter()
        .filter_map(|decl| filter_decl_to_reachable(decl, reachable))
        .collect();
    file
}

fn filter_decl_to_reachable<'a>(
    decl: crate::ast::Decl<'a>,
    reachable: &HashSet<String>,
) -> Option<crate::ast::Decl<'a>> {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            let keep = reachable.contains(func.name.name)
                || receiver_type_name(&func).is_some_and(|name| reachable.contains(&name));
            keep.then_some(crate::ast::Decl::FuncDecl(func))
        }
        crate::ast::Decl::GenDecl(mut gen_decl) => {
            if gen_decl.tok == crate::token::Token::IMPORT {
                return Some(crate::ast::Decl::GenDecl(gen_decl));
            }
            gen_decl
                .specs
                .retain(|spec| spec_names(spec).iter().any(|name| reachable.contains(name)));
            (!gen_decl.specs.is_empty()).then_some(crate::ast::Decl::GenDecl(gen_decl))
        }
    }
}

fn file_has_compilable_decl(file: &crate::ast::File<'_>) -> bool {
    file.decls.iter().any(|decl| {
        !matches!(
            decl,
            crate::ast::Decl::GenDecl(gen_decl) if gen_decl.tok == crate::token::Token::IMPORT
        )
    })
}

fn refs_from_decl(decl: &crate::ast::Decl<'_>, refs: &mut HashSet<String>) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => refs_from_func_decl(func, refs),
        crate::ast::Decl::GenDecl(gen_decl) => refs_from_gen_decl(gen_decl, refs),
    }
}

fn refs_from_gen_decl(gen_decl: &crate::ast::GenDecl<'_>, refs: &mut HashSet<String>) {
    for spec in &gen_decl.specs {
        refs_from_spec(spec, refs);
    }
}

fn refs_from_spec(spec: &crate::ast::Spec<'_>, refs: &mut HashSet<String>) {
    match spec {
        crate::ast::Spec::ImportSpec(_) => {}
        crate::ast::Spec::TypeSpec(type_spec) => {
            refs_from_field_list(type_spec.type_params.as_ref(), refs);
            refs_from_expr(&type_spec.type_, refs);
        }
        crate::ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_expr) = &value_spec.type_ {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
        }
    }
}

fn refs_from_func_decl(func: &crate::ast::FuncDecl<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func.recv.as_ref(), refs);
    refs_from_func_type(&func.type_, refs);
    if let Some(body) = &func.body {
        refs_from_block(body, refs);
    }
}

fn refs_from_func_type(func_type: &crate::ast::FuncType<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func_type.type_params.as_ref(), refs);
    refs_from_field_list(Some(&func_type.params), refs);
    refs_from_field_list(func_type.results.as_ref(), refs);
}

fn refs_from_field_list(fields: Option<&crate::ast::FieldList<'_>>, refs: &mut HashSet<String>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        if let Some(type_expr) = &field.type_ {
            refs_from_expr(type_expr, refs);
        }
    }
}

fn refs_from_block(block: &crate::ast::BlockStmt<'_>, refs: &mut HashSet<String>) {
    for stmt in &block.list {
        refs_from_stmt(stmt, refs);
    }
}

fn refs_from_stmt(stmt: &crate::ast::Stmt<'_>, refs: &mut HashSet<String>) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            refs_from_exprs(&assign.lhs, refs);
            refs_from_exprs(&assign.rhs, refs);
        }
        crate::ast::Stmt::BlockStmt(block) => refs_from_block(block, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), refs);
            for stmt in &case_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                refs_from_stmt(comm, refs);
            }
            for stmt in &comm_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => refs_from_gen_decl(&decl_stmt.decl, refs),
        crate::ast::Stmt::DeferStmt(defer_stmt) => refs_from_call(&defer_stmt.call, refs),
        crate::ast::Stmt::ExprStmt(expr_stmt) => refs_from_expr(&expr_stmt.x, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                refs_from_expr(cond, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                refs_from_stmt(post, refs);
            }
            refs_from_block(&for_stmt.body, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => refs_from_call(&go_stmt.call, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                refs_from_stmt(init, refs);
            }
            refs_from_expr(&if_stmt.cond, refs);
            refs_from_block(&if_stmt.body, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                refs_from_stmt(else_stmt, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => refs_from_expr(&inc_dec.x, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => refs_from_stmt(&labeled.stmt, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                refs_from_expr(key, refs);
            }
            if let Some(value) = &range.value {
                refs_from_expr(value, refs);
            }
            refs_from_expr(&range.x, refs);
            refs_from_block(&range.body, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => refs_from_exprs(&return_stmt.results, refs),
        crate::ast::Stmt::SelectStmt(select_stmt) => refs_from_block(&select_stmt.body, refs),
        crate::ast::Stmt::SendStmt(send_stmt) => {
            refs_from_expr(&send_stmt.chan, refs);
            refs_from_expr(&send_stmt.value, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                refs_from_expr(tag, refs);
            }
            refs_from_block(&switch_stmt.body, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            refs_from_stmt(&type_switch.assign, refs);
            refs_from_block(&type_switch.body, refs);
        }
    }
}

fn refs_from_call(call: &crate::ast::CallExpr<'_>, refs: &mut HashSet<String>) {
    refs_from_expr(&call.fun, refs);
    refs_from_exprs(call.args.as_deref().unwrap_or(&[]), refs);
}

fn refs_from_exprs(exprs: &[crate::ast::Expr<'_>], refs: &mut HashSet<String>) {
    for expr in exprs {
        refs_from_expr(expr, refs);
    }
}

fn refs_from_expr(expr: &crate::ast::Expr<'_>, refs: &mut HashSet<String>) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                refs_from_expr(len, refs);
            }
            refs_from_expr(&array.elt, refs);
        }
        crate::ast::Expr::BasicLit(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            refs_from_expr(&binary.x, refs);
            refs_from_expr(&binary.y, refs);
        }
        crate::ast::Expr::CallExpr(call) => refs_from_call(call, refs),
        crate::ast::Expr::ChanType(chan) => refs_from_expr(&chan.value, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                refs_from_expr(elt, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            refs_from_func_type(&func_lit.type_, refs);
            refs_from_block(&func_lit.body, refs);
        }
        crate::ast::Expr::FuncType(func_type) => refs_from_func_type(func_type, refs),
        crate::ast::Expr::Ident(ident) => {
            if ident.name != "_" {
                refs.insert(ident.name.to_string());
            }
        }
        crate::ast::Expr::IndexExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_expr(&index.index, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_exprs(&index.indices, refs);
        }
        crate::ast::Expr::InterfaceType(interface) => {
            refs_from_field_list(interface.methods.as_ref(), refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            refs_from_expr(&key_value.key, refs);
            refs_from_expr(&key_value.value, refs);
        }
        crate::ast::Expr::MapType(map) => {
            refs_from_expr(&map.key, refs);
            refs_from_expr(&map.value, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => refs_from_expr(&paren.x, refs),
        crate::ast::Expr::SelectorExpr(selector) => refs_from_expr(&selector.x, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            refs_from_expr(&slice.x, refs);
            if let Some(low) = slice.low.as_deref() {
                refs_from_expr(low, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                refs_from_expr(high, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                refs_from_expr(max, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => refs_from_expr(&star.x, refs),
        crate::ast::Expr::StructType(struct_type) => {
            refs_from_field_list(struct_type.fields.as_ref(), refs);
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            refs_from_expr(&type_assert.x, refs);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => refs_from_expr(&unary.x, refs),
    }
}

fn package_import_renames(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> BTreeMap<String, String> {
    let mut rewrites = BTreeMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if !is_known(import_path) {
                continue;
            }
            let mod_name = module_name(import_path);
            let Some(local_name) = import_local_name(import) else {
                continue;
            };
            if local_name != mod_name {
                rewrites.insert(local_name, mod_name);
            }
        }
    }
    rewrites
}

fn package_import_path_by_module(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if is_known(import_path) {
                imports.insert(module_name(import_path), import_path.to_string());
                if let Some(local_name) = import_local_name(import) {
                    imports.insert(local_name, import_path.to_string());
                }
            }
        }
    }
    imports
}

fn used_imports_from_items(
    items: &mut [syn::Item],
    import_path_by_module: &HashMap<String, String>,
) -> Vec<String> {
    use syn::visit_mut::VisitMut;

    struct UsedImportCollector<'a> {
        import_path_by_module: &'a HashMap<String, String>,
        used: HashSet<String>,
    }

    impl VisitMut for UsedImportCollector<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if let Some(import_path) = self.import_path_by_module.get(&first) {
                self.used.insert(import_path.clone());
            }
        }
    }

    let mut collector = UsedImportCollector {
        import_path_by_module,
        used: HashSet::new(),
    };
    for item in items {
        collector.visit_item_mut(item);
    }

    let mut used: Vec<_> = collector.used.into_iter().collect();
    used.sort();
    used
}

fn import_local_name(import: &crate::ast::ImportSpec<'_>) -> Option<String> {
    if let Some(name) = &import.name {
        if name.name == "." || name.name == "_" {
            return None;
        }
        return Some(name.name.to_string());
    }

    let import_path = import.path.value.trim_matches('"');
    scan_type_env(import_path)
        .map(|(package_name, _)| package_name)
        .or_else(|| import_path.rsplit('/').next().map(str::to_string))
}

fn import_paths_from_source(content: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_import_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import (") {
            in_import_block = true;
            continue;
        }
        if in_import_block && trimmed == ")" {
            in_import_block = false;
            continue;
        }
        if trimmed.starts_with("import ") || in_import_block {
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start + 1..].find('"') {
                    paths.push(trimmed[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }

    paths
}

fn inject_structural_helpers(items: &mut Vec<syn::Item>) {
    let has_formatter = has_trait(items, "Formatter");
    let has_stringer = has_trait(items, "Stringer");
    let has_go_stringer = has_trait(items, "GoStringer");
    let has_state = has_trait(items, "State");
    let has_pp = has_struct(items, "pp");

    if (has_formatter || has_stringer || has_go_stringer)
        && !has_struct(items, "__GorsNoopInterface")
    {
        items.insert(
            0,
            syn::parse_quote! {
                #[derive(Clone, Default)]
                struct __GorsNoopInterface;
            },
        );
    }

    if has_formatter && has_state && !has_impl(items, "Formatter", "__GorsNoopInterface") {
        items.insert(
            0,
            syn::parse_quote! {
                impl Formatter for __GorsNoopInterface {
                    fn Format(&mut self, _f: &mut dyn State, _verb: u32) {}
                }
            },
        );
    }

    if has_stringer && !has_impl(items, "Stringer", "__GorsNoopInterface") {
        items.insert(
            0,
            syn::parse_quote! {
                impl Stringer for __GorsNoopInterface {
                    fn String(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if has_go_stringer && !has_impl(items, "GoStringer", "__GorsNoopInterface") {
        items.insert(
            0,
            syn::parse_quote! {
                impl GoStringer for __GorsNoopInterface {
                    fn GoString(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if (has_stringer || has_go_stringer || has_formatter) && !has_trait(items, "__GorsErrorExt") {
        items.insert(
            0,
            syn::parse_quote! {
                trait __GorsErrorExt {
                    fn Error(&mut self) -> String;
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for __GorsNoopInterface {
                    fn Error(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if has_pp && has_state && !has_impl(items, "State", "& mut pp") {
        items.insert(
            0,
            syn::parse_quote! {
                impl<'a> State for &'a mut pp {
                    fn Write(&mut self, b: Vec<u8>) -> (isize, String) {
                        <pp as State>::Write(&mut **self, b)
                    }

                    fn Width(&mut self) -> (isize, bool) {
                        <pp as State>::Width(&mut **self)
                    }

                    fn Precision(&mut self) -> (isize, bool) {
                        <pp as State>::Precision(&mut **self)
                    }

                    fn Flag(&mut self, c: isize) -> bool {
                        <pp as State>::Flag(&mut **self, c)
                    }
                }
            },
        );
    }

    if has_pp && !has_method(items, "pp", "__gors_flush_fmt") {
        items.insert(
            0,
            syn::parse_quote! {
                impl pp {
                    fn __gors_flush_fmt(&mut self) {
                        let bytes = std::mem::take(&mut self.fmt.buf.0);
                        self.buf.0.extend(bytes);
                    }
                }
            },
        );
    }
}

fn has_trait(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Trait(item_trait) if item_trait.ident == name))
}

fn has_struct(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == name))
}

fn has_impl(items: &[syn::Item], trait_name: &str, self_ty: &str) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let Some((_, path, _)) = &item_impl.trait_ else {
            return false;
        };
        path.segments
            .last()
            .is_some_and(|seg| seg.ident == trait_name)
            && type_matches_name(&item_impl.self_ty, self_ty)
    })
}

fn type_matches_name(ty: &syn::Type, name: &str) -> bool {
    match (ty, name) {
        (syn::Type::Path(path), _) => path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == name),
        (syn::Type::Reference(reference), "& mut pp") => {
            reference.mutability.is_some() && type_matches_name(&reference.elem, "pp")
        }
        _ => false,
    }
}

fn has_method(items: &[syn::Item], ty_name: &str, method_name: &str) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let syn::Type::Path(type_path) = &*item_impl.self_ty else {
            return false;
        };
        if !type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == ty_name)
        {
            return false;
        }
        item_impl
            .items
            .iter()
            .any(|item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == method_name))
    })
}

fn prefix_crate_paths(items: &mut [syn::Item], module_refs: &HashSet<String>) {
    use syn::visit_mut::VisitMut;

    struct CratePrefixer<'a> {
        module_refs: &'a HashSet<String>,
    }

    impl VisitMut for CratePrefixer<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.leading_colon.is_some() || path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if first == "builtin" || self.module_refs.contains(&first) {
                path.segments.insert(
                    0,
                    syn::PathSegment {
                        ident: syn::Ident::new("crate", proc_macro2::Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    },
                );
            }
        }
    }

    let mut prefixer = CratePrefixer { module_refs };
    for item in items.iter_mut() {
        prefixer.visit_item_mut(item);
    }
}

fn should_compile_file(filename: &str, content: &str) -> bool {
    file_name_matches_target(filename) && build_constraint_matches(content)
}

fn file_name_matches_target(filename: &str) -> bool {
    let Some(stem) = filename.strip_suffix(".go") else {
        return false;
    };
    let parts: Vec<&str> = stem.split('_').collect();
    let Some(last) = parts.last().copied() else {
        return true;
    };

    if is_go_arch(last) {
        if last != go_arch() {
            return false;
        }
        if let Some(os_part) = parts.get(parts.len().saturating_sub(2)) {
            if is_go_os(os_part) && *os_part != go_os() {
                return false;
            }
        }
        return true;
    }

    !is_go_os(last) || last == go_os()
}

fn build_constraint_matches(content: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("//go:build ") {
            return BuildExprParser::new(expr).parse();
        }
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        break;
    }
    true
}

struct BuildExprParser<'a> {
    tokens: Vec<&'a str>,
    pos: usize,
}

impl<'a> BuildExprParser<'a> {
    fn new(expr: &'a str) -> Self {
        Self {
            tokens: tokenize_build_expr(expr),
            pos: 0,
        }
    }

    fn parse(&mut self) -> bool {
        self.parse_or()
    }

    fn parse_or(&mut self) -> bool {
        let mut value = self.parse_and();
        while self.peek() == Some("||") {
            self.pos += 1;
            value = self.parse_and() || value;
        }
        value
    }

    fn parse_and(&mut self) -> bool {
        let mut value = self.parse_unary();
        while self.peek() == Some("&&") {
            self.pos += 1;
            value = self.parse_unary() && value;
        }
        value
    }

    fn parse_unary(&mut self) -> bool {
        if self.peek() == Some("!") {
            self.pos += 1;
            return !self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> bool {
        match self.next() {
            Some("(") => {
                let value = self.parse_or();
                if self.peek() == Some(")") {
                    self.pos += 1;
                }
                value
            }
            Some(tag) => build_tag_matches(tag),
            None => true,
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.pos += 1;
        Some(token)
    }
}

fn tokenize_build_expr(expr: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;

    for (idx, ch) in expr.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            if start.is_none() {
                start = Some(idx);
            }
            continue;
        }

        if let Some(s) = start.take() {
            tokens.push(&expr[s..idx]);
        }

        match ch {
            '!' | '(' | ')' => tokens.push(&expr[idx..idx + ch.len_utf8()]),
            '&' | '|' => {
                let end = idx + 2;
                if expr.get(idx..end) == Some("&&") || expr.get(idx..end) == Some("||") {
                    tokens.push(&expr[idx..end]);
                }
            }
            _ => {}
        }
    }

    if let Some(s) = start {
        tokens.push(&expr[s..]);
    }

    tokens
}

fn build_tag_matches(tag: &str) -> bool {
    if tag == go_os() || tag == go_arch() {
        return true;
    }
    if tag == "unix" {
        return is_unix_goos(go_os());
    }
    if let Some(version) = tag.strip_prefix("go1.") {
        return version.parse::<u32>().is_ok_and(|minor| minor <= 24);
    }
    matches!(tag, "gc")
}

fn go_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => other,
    }
}

fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        other => other,
    }
}

fn is_go_os(value: &str) -> bool {
    matches!(
        value,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "js"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "plan9"
            | "solaris"
            | "wasip1"
            | "windows"
    )
}

fn is_go_arch(value: &str) -> bool {
    matches!(
        value,
        "386"
            | "amd64"
            | "arm"
            | "arm64"
            | "loong64"
            | "mips"
            | "mips64"
            | "mips64le"
            | "mipsle"
            | "ppc64"
            | "ppc64le"
            | "riscv64"
            | "s390x"
            | "sparc64"
            | "wasm"
    )
}

fn is_unix_goos(value: &str) -> bool {
    matches!(
        value,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "solaris"
    )
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}
