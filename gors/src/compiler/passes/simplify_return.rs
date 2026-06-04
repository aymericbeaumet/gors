use super::super::syn_inspect::{pat_ident_name, receiver_root_ident_name};
use std::collections::HashSet;
use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    SimplifyReturn.visit_file_mut(file);
}

struct SimplifyReturn;

impl SimplifyReturn {
    fn simplify_last_return(&self, stmts: &mut [syn::Stmt], borrowed_roots: &HashSet<String>) {
        if let Some(last) = stmts.last_mut() {
            let expr = match last {
                syn::Stmt::Expr(expr, _) => expr,
                _ => return,
            };

            if let syn::Expr::Return(ret) = expr {
                if let Some(expr) = ret.expr.take() {
                    let mut expr = *expr;
                    clone_borrowed_field_return(&mut expr, borrowed_roots);
                    *last = syn::Stmt::Expr(expr, None);
                }
            }
        }
    }
}

fn borrowed_field_roots(sig: &syn::Signature) -> HashSet<String> {
    sig.inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Receiver(receiver) if receiver.reference.is_some() => {
                Some("self".to_string())
            }
            syn::FnArg::Typed(pat_type) if matches!(&*pat_type.ty, syn::Type::Reference(_)) => {
                pat_ident_name(&pat_type.pat)
            }
            _ => None,
        })
        .collect()
}

fn clone_borrowed_field_return(expr: &mut syn::Expr, borrowed_roots: &HashSet<String>) {
    if !matches!(expr, syn::Expr::Field(_)) {
        return;
    };

    let Some(root) = receiver_root_ident_name(expr) else {
        return;
    };
    if !borrowed_roots.contains(&root) {
        return;
    }

    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
}

impl VisitMut for SimplifyReturn {
    fn visit_item_fn_mut(&mut self, item_fn: &mut syn::ItemFn) {
        let borrowed_roots = borrowed_field_roots(&item_fn.sig);
        self.simplify_last_return(&mut item_fn.block.stmts, &borrowed_roots);
    }

    fn visit_impl_item_fn_mut(&mut self, item_fn: &mut syn::ImplItemFn) {
        let borrowed_roots = borrowed_field_roots(&item_fn.sig);
        self.simplify_last_return(&mut item_fn.block.stmts, &borrowed_roots);
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::pass;
    use quote::quote;
    use syn::parse_quote;

    fn assert_pass(input: syn::File, expected: syn::File) {
        let mut output = input;
        pass(&mut output);
        assert_eq!(quote!(#output).to_string(), quote!(#expected).to_string());
    }

    #[test]
    fn borrowed_field_returns_clone_by_root_not_field_name() {
        assert_pass(
            parse_quote! {
                struct Holder {
                    text: String,
                    err: String,
                }

                impl Holder {
                    fn borrowed_self(&self) -> String {
                        return self.text;
                    }

                    fn owned_self(self) -> String {
                        return self.err;
                    }
                }

                fn borrowed_param(value: &Holder) -> String {
                    return value.text;
                }

                fn owned_param(value: Holder) -> String {
                    return value.err;
                }
            },
            parse_quote! {
                struct Holder {
                    text: String,
                    err: String,
                }

                impl Holder {
                    fn borrowed_self(&self) -> String {
                        (self.text).clone()
                    }

                    fn owned_self(self) -> String {
                        self.err
                    }
                }

                fn borrowed_param(value: &Holder) -> String {
                    (value.text).clone()
                }

                fn owned_param(value: Holder) -> String {
                    value.err
                }
            },
        );
    }
}
