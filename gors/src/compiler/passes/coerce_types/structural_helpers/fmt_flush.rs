use super::local_names::NameSet;

type ReceiverNameMap = std::collections::BTreeMap<String, NameSet>;

use crate::compiler::syn_inspect::{is_self_expr, type_path_ident_name};
use crate::generated_names::{
    FMT_FLUSH_HOOK, fmt_flush_hook_ident, fmt_flush_method_from_doc, fmt_flush_source_from_doc,
};

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
            let hook = fmt_flush_hook_ident();
            stmts.push(syn::parse_quote! {
                self.#hook();
            });
        }
    }
}

fn collect_methods_by_receiver(file: &syn::File) -> ReceiverNameMap {
    let mut methods_by_receiver = ReceiverNameMap::new();
    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_ty) = type_path_ident_name(&item_impl.self_ty) else {
            continue;
        };
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident == FMT_FLUSH_HOOK).then_some(func)
        }) {
            let methods = flush_trigger_methods(func);
            if !methods.is_empty() {
                methods_by_receiver
                    .entry(self_ty.clone())
                    .or_default()
                    .extend(methods);
            }
        }
    }
    methods_by_receiver
}

fn flush_trigger_methods(func: &syn::ImplItemFn) -> NameSet {
    if !has_flush_source_marker(func) {
        return NameSet::new();
    }
    func.attrs
        .iter()
        .filter_map(fmt_flush_method_attr)
        .collect()
}

fn has_flush_source_marker(func: &syn::ImplItemFn) -> bool {
    func.attrs
        .iter()
        .filter_map(doc_attr_value)
        .any(|doc| fmt_flush_source_from_doc(&doc).is_some())
}

fn fmt_flush_method_attr(attr: &syn::Attribute) -> Option<String> {
    let doc = doc_attr_value(attr)?;
    fmt_flush_method_from_doc(&doc).map(str::to_owned)
}

fn doc_attr_value(attr: &syn::Attribute) -> Option<String> {
    let syn::Meta::NameValue(meta) = &attr.meta else {
        return None;
    };
    if !meta.path.is_ident("doc") {
        return None;
    }
    let syn::Expr::Lit(expr_lit) = &meta.value else {
        return None;
    };
    let syn::Lit::Str(doc) = &expr_lit.lit else {
        return None;
    };
    Some(doc.value())
}

fn stmt_needs_flush(stmt: &syn::Stmt, methods: &NameSet) -> bool {
    let mut finder = FlushCallFinder {
        methods,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut finder, stmt);
    finder.found
}

struct FlushCallFinder<'a> {
    methods: &'a NameSet,
    found: bool,
}

impl syn::visit::Visit<'_> for FlushCallFinder<'_> {
    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if self.methods.contains(&call.method.to_string()) && is_self_expr(&call.receiver) {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_block(&mut self, _block: &syn::Block) {}

    fn visit_expr_closure(&mut self, _closure: &syn::ExprClosure) {}

    fn visit_expr_if(&mut self, _expr_if: &syn::ExprIf) {}

    fn visit_expr_loop(&mut self, _expr_loop: &syn::ExprLoop) {}

    fn visit_expr_match(&mut self, _expr_match: &syn::ExprMatch) {}

    fn visit_expr_while(&mut self, _expr_while: &syn::ExprWhile) {}
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
    fn metadata_reads_flush_methods_from_hook_markers() {
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
                #[doc = "gors:fmt-flush-source=inner"]
                #[doc = "gors:fmt-flush-method=emit"]
                #[doc = "gors:fmt-flush-method=run"]
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
    fn metadata_flushes_immediate_local_initializer_calls() {
        let metadata = flushable_printer_metadata(syn::parse_quote! {
            fn emit(&mut self, value: isize) -> isize {
                self.inner.write(value)
            }
        });
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            let written = self.emit(1);
        };

        assert!(metadata.should_flush_after_stmt(&receiver, &stmt));
    }

    #[test]
    fn metadata_leaves_nested_block_calls_to_nested_block_rewrite() {
        let metadata = flushable_printer_metadata(syn::parse_quote! {
            fn emit(&mut self, value: isize) {
                self.inner.write(value);
            }
        });
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            {
                self.emit(1);
            }
        };

        assert!(!metadata.should_flush_after_stmt(&receiver, &stmt));
    }

    fn flushable_printer_metadata(emit_method: syn::ImplItemFn) -> Metadata {
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
                fn write(&mut self, value: isize) -> isize { value }
            }

            impl Printer {
                #[doc = "gors:fmt-flush-source=inner"]
                #[doc = "gors:fmt-flush-method=emit"]
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                #emit_method
            }
        };

        Metadata::collect(&file)
    }

    #[test]
    fn metadata_ignores_method_names_without_hook_method_marker() {
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
                #[doc = "gors:fmt-flush-source=inner"]
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
    fn metadata_requires_generated_method_marker() {
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
                #[doc = "gors:fmt-flush-source=inner"]
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
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

    #[test]
    fn metadata_requires_generated_source_marker() {
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
                #[doc = "gors:fmt-flush-method=emit"]
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
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
