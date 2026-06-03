use syn::visit_mut::{self, VisitMut};

mod call_args;
mod comparisons;
mod evaluation_order;
mod pointer_cells;
mod static_false;
mod structural_helpers;
mod syntax;
mod tuple_newtypes;

pub fn pass(file: &mut syn::File) {
    let tuple_newtypes = tuple_newtypes::collect(file);
    let mutable_ref_call_args = call_args::collect_mutable_ref_call_args(file);
    let pointer_cell_statics = pointer_cells::collect_statics(file);
    let structural_helper_metadata = structural_helpers::Metadata::collect(file);
    CoerceTypes {
        mutable_ref_call_args,
        pointer_cell_statics,
        structural_helper_metadata,
        tuple_newtypes,
        ..Default::default()
    }
    .visit_file_mut(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    pointer_cells::pass_after_package_merge(file);
}

pub fn pass_after_structural_helpers(file: &mut syn::File) {
    structural_helpers::pass_after_structural_helpers(file);
}

#[derive(Default)]
struct CoerceTypes {
    call_arg_scopes: Vec<call_args::FnArgScope>,
    mutable_ref_call_args: call_args::MutableRefCallArgs,
    pointer_cell_statics: pointer_cells::StaticNames,
    structural_helper_metadata: structural_helpers::Metadata,
    tuple_newtypes: tuple_newtypes::Names,
    impl_self_types: Vec<String>,
}

impl VisitMut for CoerceTypes {
    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        if let Some(self_ty) = syntax::type_path_ident_name(&item_impl.self_ty) {
            self.impl_self_types.push(self_ty);
            visit_mut::visit_item_impl_mut(self, item_impl);
            self.impl_self_types.pop();
        } else {
            visit_mut::visit_item_impl_mut(self, item_impl);
        }
    }

    fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
        let scope = call_args::FnArgScope::collect(&func.sig);
        self.call_arg_scopes.push(scope);
        visit_mut::visit_item_fn_mut(self, func);
        self.call_arg_scopes.pop();

        static_false::prune_branches(&mut func.block.stmts);
        structural_helpers::prune_reflection_fallback(&mut func.block.stmts, None);
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        let scope = call_args::FnArgScope::collect(&func.sig);
        self.call_arg_scopes.push(scope);
        visit_mut::visit_impl_item_fn_mut(self, func);
        self.call_arg_scopes.pop();

        static_false::prune_branches(&mut func.block.stmts);
        let self_reflect_fields = self.impl_self_types.last().and_then(|ty| {
            self.structural_helper_metadata
                .self_reflect_fields_for_initial_pass(ty, &func.block)
        });
        structural_helpers::prune_reflection_fallback(&mut func.block.stmts, self_reflect_fields);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            new_stmts.extend(evaluation_order::hoist_args_read_after_mut_borrow(
                &mut stmt,
            ));
            new_stmts
                .extend(evaluation_order::hoist_condition_args_read_after_mut_borrow(&mut stmt));
            new_stmts.extend(evaluation_order::hoist_method_args_read_receiver(&mut stmt));
            self.structural_helper_metadata.push_stmt_with_flush(
                &self.impl_self_types,
                stmt,
                &mut new_stmts,
            );
        }

        block.stmts = new_stmts;
    }

    fn visit_expr_method_call_mut(&mut self, mc: &mut syn::ExprMethodCall) {
        visit_mut::visit_expr_method_call_mut(self, mc);
        call_args::coerce_scoped_call_args(&mut mc.args, self.call_arg_scopes.last());
    }

    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        visit_mut::visit_expr_binary_mut(self, binary);
        comparisons::coerce_binary_expr(binary);
    }

    fn visit_expr_assign_mut(&mut self, assign: &mut syn::ExprAssign) {
        visit_mut::visit_expr_assign_mut(self, assign);
        tuple_newtypes::coerce_assignment(assign, &self.impl_self_types, &self.tuple_newtypes);
    }

    fn visit_expr_cast_mut(&mut self, cast: &mut syn::ExprCast) {
        visit_mut::visit_expr_cast_mut(self, cast);
        tuple_newtypes::coerce_cast(cast, &self.impl_self_types, &self.tuple_newtypes);
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        visit_mut::visit_expr_call_mut(self, call);
        tuple_newtypes::coerce_numeric_from_call(call, &self.impl_self_types, &self.tuple_newtypes);
        call_args::coerce_scoped_call_args(&mut call.args, self.call_arg_scopes.last());
        call_args::coerce_signature_call_args(
            &call.func,
            &mut call.args,
            &self.mutable_ref_call_args,
            &self.pointer_cell_statics,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_prunes_reflection_fallback_inside_generated_block() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            '__gors_switch: {
                let mut __gors_switch_selected: isize = -1;
                if __gors_switch_selected == -1 {
                    __gors_switch_selected = 0;
                }
                if __gors_switch_selected == 0 {
                    self.fmt.fmtBs(v);
                }
                if __gors_switch_selected == 1 {
                    self.printValue(crate::reflect::ValueOf(v), verb, 0);
                }
            };
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, None);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            tokens.contains("fmtBs"),
            "expected non-reflection switch case to remain: {tokens}"
        );
        assert!(
            !tokens.contains("printValue"),
            "expected reflection fallback to be pruned: {tokens}"
        );
        assert!(
            !tokens.contains("crate :: reflect"),
            "expected reflect dependency to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_reflect_mentions_inside_literals() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            let msg = "crate :: reflect :: ValueOf";
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, None);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            tokens.contains("let msg"),
            "expected string-literal reflect mention to remain: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_local_identifiers_named_reflect() {
        let mut stmts: Vec<syn::Stmt> = vec![
            syn::parse_quote! {
                let reflect = 1;
            },
            syn::parse_quote! {
                let value = reflect + 1;
            },
        ];

        structural_helpers::prune_reflection_fallback(&mut stmts, None);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            tokens.contains("let reflect") && tokens.contains("reflect + 1"),
            "expected local identifier named reflect to remain: {tokens}"
        );
    }

    #[test]
    fn it_prunes_unqualified_reflect_module_paths() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            reflect::ValueOf(v);
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, None);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            !tokens.contains("reflect :: ValueOf"),
            "expected reflect module path to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_prunes_reflect_type_paths_inside_generated_fallbacks() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            value.is::<crate::reflect::Value>();
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, None);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            !tokens.contains("crate :: reflect"),
            "expected reflect type-path fallback to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_named_bodies_from_literal_mentions() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct pp;

            pub fn newPrinter() -> String {
                "ppFree".to_string()
            }

            pub struct Sink;

            impl Sink {
                pub fn fmtString(&mut self, mut v: String) {
                    let marker = "fmtQ";
                    let _ = (marker, v);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("to_string") && tokens.contains("let marker"),
            "expected literal mentions not to trigger body replacements: {tokens}"
        );
        assert!(
            !tokens.contains("pp :: default") && !tokens.contains("fmtS (v)"),
            "expected no token-string-driven body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_pad_string_body_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Buffer;

            impl Buffer {
                pub fn writeString(&mut self, mut s: String) {}
            }

            pub struct fmt {
                pub buf: Buffer,
                pub widPresent: bool,
                pub wid: isize,
                pub minus: bool,
            }

            pub fn RuneCountInString(mut s: String) -> isize {
                0
            }

            impl fmt {
                pub fn writePadding(&mut self, mut width: isize) {}

                pub fn padString(&mut self, mut s: String) {
                    if !self.widPresent || self.wid == 0 {
                        self.buf.writeString((s).clone());
                        return;
                    }
                    let width = self.wid - RuneCountInString((s).clone());
                    if !self.minus {
                        self.writePadding(width);
                        self.buf.writeString((s).clone());
                    } else {
                        self.buf.writeString((s).clone());
                        self.writePadding(width);
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("RuneCountInString") && tokens.contains("writePadding"),
            "expected padString body to remain generic lowering output: {tokens}"
        );
        assert!(
            !tokens.contains("lock () . unwrap"),
            "expected no named padString body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_fmt_string_body_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct rawfmt;

            impl rawfmt {
                pub fn fmtS(&mut self, mut s: String) {}
                pub fn fmtQ(&mut self, mut s: String) {}
                pub fn fmtSx(&mut self, mut s: String, mut digits: String) {}
            }

            pub struct pp {
                pub fmt: rawfmt,
            }

            impl pp {
                pub fn fmtString(&mut self, mut v: String, mut verb: i32) {
                    if verb == 113 {
                        self.fmt.fmtQ((v).clone());
                    } else if verb == 120 {
                        self.fmt.fmtSx((v).clone(), "0123456789abcdefx".to_string());
                    } else {
                        self.fmt.fmtS((v).clone());
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("fmtQ") && tokens.contains("fmtSx"),
            "expected fmtString branch body to remain generic lowering output: {tokens}"
        );
        assert!(
            !tokens.contains("self . fmt . fmtS (v)"),
            "expected no named fmtString body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_non_fmt_self_value_statements() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Setting {
                pub value: isize,
            }

            impl Setting {
                pub fn Value(&mut self) -> isize {
                    let mut v = self.value;
                    if v > 0 {
                        return v;
                    }
                    v
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let mut v = self . value"),
            "expected ordinary self.value local binding to remain: {tokens}"
        );
    }

    #[test]
    fn it_inserts_flush_for_receivers_with_generated_flush_hook() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub inner: Inner,
                pub buf: Buffer,
            }

            pub struct Inner {
                pub buf: Buffer,
            }

            pub struct Buffer(pub Vec<u8>);

            impl Inner {
                pub fn write(&mut self, value: isize) {}
            }

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                pub fn emit(&mut self, value: isize) {
                    self.inner.write(value);
                }

                pub fn run(&mut self) {
                    self.emit(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . emit (1) ; self . __gors_flush_fmt ()"),
            "expected generated flush hook after method using the hook source field: {tokens}"
        );
    }

    #[test]
    fn it_inserts_flush_after_structural_helpers_are_injected() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub inner: Inner,
                pub buf: Buffer,
            }

            pub struct Inner {
                pub buf: Buffer,
            }

            pub struct Buffer(pub Vec<u8>);

            impl Inner {
                pub fn write(&mut self, value: isize) {}
            }

            impl Printer {
                pub fn emit(&mut self, value: isize) {
                    self.inner.write(value);
                }

                pub fn run(&mut self) {
                    self.emit(1);
                }
            }

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }
            }
        };

        pass_after_structural_helpers(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . emit (1) ; self . __gors_flush_fmt ()"),
            "expected generated flush hook after structural helpers are injected: {tokens}"
        );
    }

    #[test]
    fn it_does_not_insert_flush_for_receivers_without_generated_flush_hook() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer;

            impl Printer {
                pub fn printArg(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("__gors_flush_fmt"),
            "expected no flush without generated flush hook: {tokens}"
        );
    }

    #[test]
    fn it_does_not_insert_flush_by_method_name_alone() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub inner: Inner,
                pub buf: Buffer,
            }

            pub struct Inner {
                pub buf: Buffer,
            }

            pub struct Buffer(pub Vec<u8>);

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                pub fn printArg(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . printArg (1)")
                && !tokens.contains("self . printArg (1) ; self . __gors_flush_fmt ()"),
            "expected flush insertion to require source-field use, not a method name: {tokens}"
        );
    }

    #[test]
    fn it_prunes_self_value_reflection_fallback_from_generated_flush_receivers() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub inner: Inner,
                pub buf: Buffer,
                pub value: crate::reflect::Value,
            }

            pub struct Inner {
                pub buf: Buffer,
            }

            pub struct Buffer(pub Vec<u8>);

            impl Inner {
                pub fn write(&mut self, value: isize) {}
            }

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                pub fn printValue(&mut self, value: crate::reflect::Value) {
                    self.inner.write(0);
                }

                pub fn run(&mut self) {
                    let mut fallback = self.value;
                    self.printValue(fallback);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("fallback") && !tokens.contains("self . value"),
            "expected generated self.value reflection fallback to be pruned by receiver metadata: {tokens}"
        );
    }

    #[test]
    fn it_prunes_dependents_of_reflect_field_locals() {
        let mut stmts: Vec<syn::Stmt> = vec![
            syn::parse_quote! {
                let mut fallback = self.value;
            },
            syn::parse_quote! {
                self.printValue(fallback);
            },
        ];
        let fields = std::collections::HashSet::from(["value".to_string()]);

        structural_helpers::prune_reflection_fallback(&mut stmts, Some(&fields));

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            !tokens.contains("fallback") && !tokens.contains("printValue"),
            "expected reflect-field local and its dependent call to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_prunes_self_value_reflection_fallback_without_print_call() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub arg: Box<dyn std::any::Any>,
                pub value: crate::reflect::Value,
                pub buf: Buffer,
            }

            pub struct Buffer;

            impl Buffer {
                pub fn writeByte(&mut self, value: u8) {}
                pub fn writeString(&mut self, value: String) {}
            }

            pub fn nilAngleString() -> String {
                "nil".to_string()
            }

            impl Printer {
                pub fn badVerb(&mut self) {
                    if !crate::builtin::interface_is_nil(
                        (crate::builtin::clone_any(&self.arg)).as_ref(),
                    ) {
                        self.buf.writeByte(61u8);
                        let __gors_premethod_arg_0 = crate::builtin::clone_any(&self.arg);
                    } else if (self.value).clone().IsValid() {
                        let __gors_premethod_arg_0 = (self.value).clone().Type().String();
                        self.buf.writeString((__gors_premethod_arg_0).clone());
                    } else {
                        self.buf.writeString(nilAngleString());
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("IsValid") && !tokens.contains("Type"),
            "expected generated self.value reflection fallback to be pruned without a print call: {tokens}"
        );
        assert!(
            tokens.contains("nilAngleString"),
            "expected non-reflection fallback branch to remain: {tokens}"
        );
    }

    #[test]
    fn it_does_not_clone_local_initializers_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct State {
                pub fmtFlags: isize,
            }

            pub fn use_names(mut value: isize, mut f: isize, mut state: State) -> isize {
                let mut from_value = value;
                let mut from_f = f;
                let mut from_field = state.fmtFlags;
                from_value + from_f + from_field
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let mut from_value = value"),
            "expected local named value not to force cloning: {tokens}"
        );
        assert!(
            tokens.contains("let mut from_f = f"),
            "expected local named f not to force cloning: {tokens}"
        );
        assert!(
            tokens.contains("let mut from_field = state . fmtFlags"),
            "expected field named fmtFlags not to force cloning: {tokens}"
        );
        assert!(
            !tokens.contains("value) . clone") && !tokens.contains("f) . clone"),
            "expected no identifier-name-driven local clones: {tokens}"
        );
    }

    #[test]
    fn it_does_not_clone_arbitrary_box_new_field_args() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct State {
                pub value: String,
            }

            pub fn call(mut state: State) -> Box<String> {
                Box::new(state.value)
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("Box :: new (state . value)"),
            "expected arbitrary Box::new field argument to stay untouched: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new ((state . value) . clone ())"),
            "expected no broad boxed-field clone rewrite: {tokens}"
        );
    }

    #[test]
    fn it_does_not_coerce_print_value_args_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Pair {
                pub Key: isize,
                pub Value: isize,
            }

            pub struct Sink;

            impl Sink {
                pub fn printValue(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut pair: Pair) {
                sink.printValue(pair.Key);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printValue (pair . Key)"),
            "expected method and field names not to force reflect coercion: {tokens}"
        );
        assert!(
            !tokens.contains("reflect :: ValueOf"),
            "expected no method-name-driven reflect coercion: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_self_print_value_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink;

            impl Sink {
                pub fn printValue(&mut self, value: isize) {}

                pub fn call(&mut self) {
                    self.printValue(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . printValue (1)"),
            "expected self.printValue without reflect data to remain: {tokens}"
        );
    }

    #[test]
    fn it_does_not_box_print_arg_err_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink;

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut err: isize) {
                sink.printArg(err);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printArg (err)"),
            "expected local name err not to force boxing: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (err)"),
            "expected no method-name-driven err boxing: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_print_arg_index_args_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink;

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut values: Vec<isize>) {
                sink.printArg(values[0]);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printArg (values [0])"),
            "expected method name not to force indexed argument replacement: {tokens}"
        );
        assert!(
            !tokens.contains("std :: mem :: replace"),
            "expected no method-name-driven indexed argument replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_print_arg_self_arg_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                arg: isize,
            }

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}

                pub fn call(&mut self) {
                    self.printArg(self.arg);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let __gors_premethod_arg_0 = self . arg")
                && tokens.contains("self . printArg (__gors_premethod_arg_0)"),
            "expected method and field names not to force empty any replacement: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (())"),
            "expected no method-name-driven empty any replacement: {tokens}"
        );
    }

    #[test]
    fn it_hoists_args_that_read_locked_receiver_root() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call(mut p: P) {
                (|| {
                    (p.lock().unwrap().fmt).init(crate::builtin::GorsPtr::new({
                        let __gors_pointer_field = (p.lock().unwrap().buf).clone();
                        __gors_pointer_field
                    }));
                })();
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        let hoist_pos =
            tokens.find("let __gors_premethod_arg_0 = crate :: builtin :: GorsPtr :: new");
        assert!(
            hoist_pos.is_some(),
            "expected locked receiver argument to be hoisted: {tokens}"
        );
        let hoist_pos = hoist_pos.unwrap_or_default();
        let call_pos =
            tokens.find("(p . lock () . unwrap () . fmt) . init (__gors_premethod_arg_0)");
        assert!(
            call_pos.is_some(),
            "expected method call to use hoisted argument: {tokens}"
        );
        let call_pos = call_pos.unwrap_or_default();
        assert!(
            hoist_pos < call_pos,
            "expected argument to be evaluated before locked receiver call: {tokens}"
        );
    }

    #[test]
    fn it_hoists_condition_args_read_after_mut_borrow() {
        let mut file: syn::File = syn::parse_quote! {
            pub trait Interface {
                fn Len(&mut self) -> isize;
            }

            pub fn down(h: &mut dyn Interface, i: isize, n: isize) -> bool {
                false
            }

            pub fn up(h: &mut dyn Interface, i: isize) {}

            pub fn fix(mut h: &mut dyn Interface, mut i: isize) {
                if !down(&mut *h, i, h.Len()) {
                    up(&mut *h, i);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let __gors_preborrow_arg_0 = h . Len ()")
                && tokens.contains("down (& mut * h , i , __gors_preborrow_arg_0)"),
            "expected condition argument read after mutable borrow to be hoisted: {tokens}"
        );
    }

    #[test]
    fn it_does_not_rewrite_err_assignment_from_w_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call(mut err: isize, mut w: isize) {
                err = w;
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("err = w"),
            "expected local names not to force field extraction: {tokens}"
        );
        assert!(
            !tokens.contains("w . lock () . unwrap () . err"),
            "expected no method-name-driven err field extraction: {tokens}"
        );
    }

    #[test]
    fn it_does_not_rewrite_self_arg_assignment_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                arg: isize,
            }

            impl Sink {
                pub fn save(&mut self, mut arg: isize) {
                    self.arg = arg;
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . arg = arg"),
            "expected self.arg assignment to remain name independent: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (())"),
            "expected no field-name-driven empty any replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_clone_self_value_assignment_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                value: isize,
            }

            impl Sink {
                pub fn save(&mut self, mut value: isize) {
                    self.value = value;
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . value = value"),
            "expected self.value assignment to remain name independent: {tokens}"
        );
        assert!(
            !tokens.contains("(value) . clone"),
            "expected no field-name-driven value clone: {tokens}"
        );
    }

    #[test]
    fn it_casts_tuple_newtype_self_through_inner_field() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct KeySizeError(pub isize);

            impl KeySizeError {
                pub fn Error(&self) -> String {
                    crate::strconv::Itoa((self as isize))
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . 0 as isize"),
            "expected tuple newtype receiver casts to use the inner field: {tokens}"
        );
        assert!(
            !tokens.contains("self as isize"),
            "expected borrowed receiver cast to be rewritten: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_static_pointees_for_mut_ref_calls() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub static Public: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Table>>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Table { n: 1 })));

            fn check(mut table: &mut Table) -> isize {
                table.n
            }

            pub fn call() -> isize {
                check((*Public).clone())
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * ((* Public)) . lock () . unwrap ())"),
            "expected pointer-cell static to be locked for mutable-reference call: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut (* Public) . clone ())"),
            "expected not to borrow a cloned Arc cell: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_static_pointees_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call() -> isize {
                check((*Public).clone())
            }

            fn check(mut table: &mut Table) -> isize {
                table.n
            }

            pub static Public: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Table>>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Table { n: 1 })));

            pub struct Table {
                pub n: isize,
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * ((* Public)) . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell static: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut (* Public) . clone ())"),
            "expected post-merge pass not to borrow a cloned Arc cell: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_range_locals_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub fn any(mut tables: Vec<std::sync::Arc<std::sync::Mutex<Table>>>) -> bool {
                for (_, mut table) in (tables)
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, v)| (i as isize, v))
                {
                    if check(&mut table) {
                        return true;
                    }
                }
                false
            }

            fn check(mut table: &mut Table) -> bool {
                table.n > 0
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * table . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell range local: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut table)"),
            "expected post-merge pass not to pass the pointer cell itself: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_value_params_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub fn call(mut table: std::sync::Arc<std::sync::Mutex<Table>>) -> bool {
                check(table)
            }

            fn check(mut table: &mut Table) -> bool {
                table.n > 0
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * table . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell value parameter: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut table)"),
            "expected post-merge pass not to pass the pointer cell itself: {tokens}"
        );
    }
}
