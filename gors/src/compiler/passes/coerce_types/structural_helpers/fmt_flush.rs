type NameSet = std::collections::HashSet<String>;
type ReceiverNameMap = std::collections::BTreeMap<String, NameSet>;

#[derive(Default)]
pub(super) struct Metadata {
    methods_by_receiver: ReceiverNameMap,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            methods_by_receiver: collect_methods_by_receiver(file),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.methods_by_receiver.is_empty()
    }

    fn should_flush_after_stmt(&self, impl_self_types: &[String], stmt: &syn::Stmt) -> bool {
        let Some(methods) = impl_self_types
            .last()
            .and_then(|ty| self.methods_by_receiver.get(ty))
        else {
            return false;
        };
        stmt_needs_flush(stmt, methods)
    }

    pub(super) fn push_stmt_with_flush(
        &self,
        impl_self_types: &[String],
        stmt: syn::Stmt,
        stmts: &mut Vec<syn::Stmt>,
    ) {
        let needs_flush = self.should_flush_after_stmt(impl_self_types, &stmt);
        stmts.push(stmt);
        if needs_flush {
            stmts.push(syn::parse_quote! {
                self.__gors_flush_fmt();
            });
        }
    }
}

fn collect_methods_by_receiver(file: &syn::File) -> ReceiverNameMap {
    let flush_source_fields = collect_source_fields_by_receiver(file);
    let mut methods_by_receiver = ReceiverNameMap::new();

    for (self_ty, source_fields) in flush_source_fields {
        let receiver_methods = collect_receiver_methods(file, &self_ty, &source_fields);
        if !receiver_methods.is_empty() {
            methods_by_receiver.insert(self_ty, receiver_methods);
        }
    }
    methods_by_receiver
}

fn collect_source_fields_by_receiver(file: &syn::File) -> ReceiverNameMap {
    let mut fields_by_receiver = ReceiverNameMap::new();

    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_ty) = super::super::syntax::type_path_ident_name(&item_impl.self_ty) else {
            continue;
        };
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident == "__gors_flush_fmt").then_some(func)
        }) {
            let fields = flush_source_fields(func);
            if !fields.is_empty() {
                fields_by_receiver
                    .entry(self_ty.clone())
                    .or_default()
                    .extend(fields);
            }
        }
    }

    fields_by_receiver
}

fn collect_receiver_methods(file: &syn::File, self_ty: &str, source_fields: &NameSet) -> NameSet {
    let mut direct_methods = NameSet::new();
    let mut calls_by_method = std::collections::BTreeMap::<String, NameSet>::new();

    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if super::super::syntax::type_path_ident_name(&item_impl.self_ty).as_deref()
            != Some(self_ty)
        {
            continue;
        }
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident != "__gors_flush_fmt").then_some(func)
        }) {
            let name = func.sig.ident.to_string();
            calls_by_method
                .entry(name.clone())
                .or_default()
                .extend(self_method_calls(func));
            if method_calls_flush_source_field(func, source_fields) {
                direct_methods.insert(name);
            }
        }
    }

    expand_transitive_methods(direct_methods, &calls_by_method)
}

fn expand_transitive_methods(
    mut methods: NameSet,
    calls_by_method: &std::collections::BTreeMap<String, NameSet>,
) -> NameSet {
    loop {
        let mut changed = false;
        for (method, callees) in calls_by_method {
            if methods.contains(method) || !callees.iter().any(|callee| methods.contains(callee)) {
                continue;
            }
            methods.insert(method.clone());
            changed = true;
        }
        if !changed {
            break;
        }
    }
    methods
}

fn flush_source_fields(func: &syn::ImplItemFn) -> NameSet {
    struct Finder {
        fields: NameSet,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_call(&mut self, call: &syn::ExprCall) {
            if super::super::syntax::is_path_call(call.func.as_ref(), &["std", "mem", "take"]) {
                for arg in &call.args {
                    super::self_fields::collect_direct_self_fields(arg, &mut self.fields);
                }
                return;
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    let mut finder = Finder {
        fields: NameSet::new(),
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.fields
}

fn self_method_calls(func: &syn::ImplItemFn) -> NameSet {
    struct Finder {
        calls: NameSet,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if super::super::syntax::is_self_expr(&call.receiver) {
                self.calls.insert(call.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        calls: NameSet::new(),
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.calls
}

fn method_calls_flush_source_field(func: &syn::ImplItemFn, source_fields: &NameSet) -> bool {
    struct Finder<'a> {
        source_fields: &'a NameSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if super::self_fields::expr_mentions(&call.receiver, self.source_fields) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        source_fields,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.found
}

fn stmt_needs_flush(stmt: &syn::Stmt, methods: &NameSet) -> bool {
    matches!(stmt, syn::Stmt::Expr(expr, _) if expr_needs_flush(expr, methods))
}

fn expr_needs_flush(expr: &syn::Expr, methods: &NameSet) -> bool {
    let syn::Expr::MethodCall(call) = expr else {
        return false;
    };
    if !methods.contains(&call.method.to_string()) {
        return false;
    }
    super::super::syntax::is_self_expr(&call.receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_requires_hook_on_same_receiver() {
        let file: syn::File = syn::parse_quote! {
            struct Printer;
            struct Other;

            impl Printer {
                fn printArg(&mut self) {}
            }

            impl Other {
                fn __gors_flush_fmt(&mut self) {}
            }
        };

        let metadata = Metadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            self.printArg();
        };

        assert!(!metadata.should_flush_after_stmt(&receiver, &stmt));
    }

    #[test]
    fn metadata_derives_methods_from_hook_source_field() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                inner: Inner,
                buf: Buffer,
            }

            struct Inner {
                buf: Buffer,
            }

            struct Buffer(Vec<u8>);

            impl Inner {
                fn write(&mut self, value: isize) {}
            }

            impl Printer {
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                fn emit(&mut self, value: isize) {
                    self.inner.write(value);
                }

                fn run(&mut self) {
                    self.emit(1);
                }
            }
        };

        let metadata = Metadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let emit_stmt: syn::Stmt = syn::parse_quote! {
            self.emit(1);
        };
        let run_stmt: syn::Stmt = syn::parse_quote! {
            self.run();
        };

        assert!(metadata.should_flush_after_stmt(&receiver, &emit_stmt));
        assert!(metadata.should_flush_after_stmt(&receiver, &run_stmt));
    }

    #[test]
    fn metadata_ignores_method_names_without_source_field_use() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                inner: Inner,
                buf: Buffer,
            }

            struct Inner {
                buf: Buffer,
            }

            struct Buffer(Vec<u8>);

            impl Printer {
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                fn printArg(&mut self, value: isize) {}

                fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        let metadata = Metadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            self.printArg(1);
        };

        assert!(!metadata.should_flush_after_stmt(&receiver, &stmt));
    }

    #[test]
    fn metadata_requires_generated_std_mem_take_source() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                inner: Inner,
                buf: Buffer,
            }

            struct Inner {
                buf: Buffer,
            }

            struct Buffer(Vec<u8>);

            impl Inner {
                fn write(&mut self, value: isize) {}
            }

            impl Printer {
                fn __gors_flush_fmt(&mut self) {
                    let bytes = take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                fn emit(&mut self, value: isize) {
                    self.inner.write(value);
                }
            }
        };

        let metadata = Metadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            self.emit(1);
        };

        assert!(!metadata.should_flush_after_stmt(&receiver, &stmt));
    }
}
