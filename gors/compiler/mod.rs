mod passes;

use crate::{ast, token};
use proc_macro2::Span;
use syn::visit_mut::VisitMut;
use syn::Token;

pub fn compile(file: ast::File) -> Result<syn::File, Box<dyn std::error::Error>> {
    let mut out = file.into();

    passes::InlineFmt.visit_file_mut(&mut out);

    Ok(out)
}

impl From<ast::BasicLit<'_>> for syn::ExprLit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        syn::ExprLit {
            attrs: vec![],
            lit: basic_lit.into(),
        }
    }
}

impl From<ast::BasicLit<'_>> for syn::Lit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        use token::Token::*;
        match basic_lit.kind {
            INT => syn::Lit::Int(syn::LitInt::new(basic_lit.value, Span::mixed_site())),
            STRING => {
                let mut value = basic_lit.value.chars();
                value.next();
                value.next_back();
                syn::Lit::Str(syn::LitStr::new(value.as_str(), Span::mixed_site()))
            }
            _ => unimplemented!("{:?}", basic_lit),
        }
    }
}

impl From<ast::BinaryExpr<'_>> for syn::ExprBinary {
    fn from(binary_expr: ast::BinaryExpr) -> Self {
        syn::ExprBinary {
            attrs: vec![],
            left: Box::new((*binary_expr.x).into()),
            op: binary_expr.op.into(),
            right: Box::new((*binary_expr.y).into()),
        }
    }
}

impl From<ast::BlockStmt<'_>> for syn::Block {
    fn from(block_stmt: ast::BlockStmt) -> Self {
        syn::Block {
            brace_token: syn::token::Brace {
                span: Span::mixed_site(),
            },
            stmts: block_stmt
                .list
                .into_iter()
                .map(|stmt| stmt.into())
                .collect(),
        }
    }
}

impl From<ast::BlockStmt<'_>> for syn::ExprBlock {
    fn from(block_stmt: ast::BlockStmt) -> Self {
        syn::ExprBlock {
            attrs: vec![],
            label: None,
            block: block_stmt.into(),
        }
    }
}

impl From<ast::CallExpr<'_>> for syn::ExprCall {
    fn from(call_expr: ast::CallExpr) -> Self {
        let mut args = syn::punctuated::Punctuated::new();
        if let Some(cargs) = call_expr.args {
            for arg in cargs {
                args.push(arg.into())
            }
        }

        Self {
            attrs: vec![],
            func: Box::new((*call_expr.fun).into()),
            paren_token: syn::token::Paren {
                span: Span::mixed_site(),
            },
            args,
        }
    }
}

impl From<ast::Expr<'_>> for syn::Expr {
    fn from(expr: ast::Expr) -> Self {
        match expr {
            ast::Expr::BasicLit(basic_lit) => syn::Expr::Lit(basic_lit.into()),
            ast::Expr::BinaryExpr(binary_expr) => syn::Expr::Binary(binary_expr.into()),
            ast::Expr::CallExpr(call_expr) => syn::Expr::Call(call_expr.into()),
            ast::Expr::Ident(ident) => syn::Expr::Path(ident.into()),
            ast::Expr::SelectorExpr(selector_expr) => syn::Expr::Path(selector_expr.into()),
            _ => unimplemented!("{:?}", expr),
        }
    }
}

impl From<ast::Expr<'_>> for syn::Type {
    fn from(expr: ast::Expr) -> Self {
        match expr {
            ast::Expr::Ident(ident) => {
                let mut segments = syn::punctuated::Punctuated::new();
                segments.push(syn::PathSegment {
                    ident: syn::Ident::new(
                        match ident.name {
                            "bool" => "bool",
                            "rune" => "u32",
                            "string" => "String",
                            "float32" => "f32",
                            "float64" => "f64",
                            "int" => "isize",
                            "int8" => "i8",
                            "int16" => "i16",
                            "int32" => "i32",
                            "int64" => "i64",
                            "uint" => "usize",
                            "uint8" => "u8",
                            "uint16" => "u16",
                            "uint32" => "u32",
                            "uint64" => "u64",
                            _ => unimplemented!("no support for type {:?} yet", ident.name),
                        },
                        Span::mixed_site(),
                    ),
                    arguments: syn::PathArguments::None,
                });
                syn::Type::Path(syn::TypePath {
                    qself: None,
                    path: syn::Path {
                        leading_colon: None,
                        segments,
                    },
                })
            }
            _ => unimplemented!("{:?}", expr),
        }
    }
}

impl From<ast::File<'_>> for syn::File {
    fn from(file: ast::File) -> Self {
        let items = file
            .decls
            .into_iter()
            .filter_map(|decl| match decl {
                ast::Decl::FuncDecl(func_decl) => Some(syn::Item::Fn(func_decl.into())),
                _ => None,
            })
            .collect();

        Self {
            attrs: vec![],
            items,
            shebang: None,
        }
    }
}

impl From<ast::Field<'_>> for syn::FnArg {
    fn from(field: ast::Field) -> Self {
        let name = field.names.unwrap().into_iter().next().unwrap();
        Self::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: None,
                ident: name.into(),
            })),
            colon_token: <Token![:]>::default(),
            ty: Box::new(field.type_.unwrap().into()),
        })
    }
}

impl From<ast::FuncDecl<'_>> for syn::ItemFn {
    fn from(func_decl: ast::FuncDecl) -> Self {
        let mut inputs = syn::punctuated::Punctuated::new();
        for param in func_decl.type_.params.list {
            inputs.push(param.into());
        }

        let vis = (&func_decl.name).into();

        let block =
            Box::new(
                func_decl
                    .body
                    .map(|body| body.into())
                    .unwrap_or_else(|| syn::Block {
                        brace_token: syn::token::Brace {
                            span: Span::mixed_site(),
                        },
                        stmts: vec![],
                    }),
            );

        let sig = syn::Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            fn_token: <Token![fn]>::default(),
            ident: func_decl.name.into(),
            generics: syn::Generics {
                params: syn::punctuated::Punctuated::new(),
                lt_token: None,
                gt_token: None,
                where_clause: None,
            },
            paren_token: syn::token::Paren {
                span: Span::mixed_site(),
            },
            inputs,
            variadic: None,
            output: syn::ReturnType::Default,
        };

        Self {
            attrs: vec![],
            block,
            sig,
            vis,
        }
    }
}

impl From<ast::Ident<'_>> for syn::ExprPath {
    fn from(ident: ast::Ident) -> Self {
        let mut segments = syn::punctuated::Punctuated::new();
        segments.push(syn::PathSegment {
            ident: ident.into(),
            arguments: syn::PathArguments::None,
        });

        Self {
            attrs: vec![],
            path: syn::Path {
                leading_colon: None,
                segments,
            },
            qself: None,
        }
    }
}

impl From<&ast::Ident<'_>> for syn::Visibility {
    fn from(name: &ast::Ident) -> Self {
        if name.name == "main" || matches!(name.name.chars().next(), Some('A'..='Z')) {
            syn::Visibility::Public(syn::VisPublic {
                pub_token: <Token![pub]>::default(),
            })
        } else {
            syn::Visibility::Inherited
        }
    }
}

impl From<ast::SelectorExpr<'_>> for syn::ExprPath {
    fn from(selector_expr: ast::SelectorExpr) -> Self {
        let x = match *selector_expr.x {
            ast::Expr::Ident(ident) => ident,
            _ => unimplemented!(),
        };

        let mut segments = syn::punctuated::Punctuated::new();
        segments.push(syn::PathSegment {
            ident: x.into(),
            arguments: syn::PathArguments::None,
        });
        segments.push(syn::PathSegment {
            ident: selector_expr.sel.into(),
            arguments: syn::PathArguments::None,
        });

        Self {
            attrs: vec![],
            path: syn::Path {
                leading_colon: None,
                segments,
            },
            qself: None,
        }
    }
}

impl From<ast::Ident<'_>> for syn::Ident {
    fn from(ident: ast::Ident) -> Self {
        Self::new(ident.name, Span::mixed_site())
    }
}

impl From<ast::IfStmt<'_>> for syn::ExprIf {
    fn from(if_stmt: ast::IfStmt) -> Self {
        syn::ExprIf {
            attrs: vec![],
            cond: Box::new(if_stmt.cond.into()),
            if_token: <Token![if]>::default(),
            then_branch: if_stmt.body.into(),
            else_branch: if_stmt.else_.map(|else_| {
                (
                    <Token![else]>::default(),
                    Box::new(match else_ {
                        ast::Stmt::IfStmt(if_stmt) => syn::Expr::If(if_stmt.into()),
                        ast::Stmt::BlockStmt(block_stmt) => syn::Expr::Block(block_stmt.into()),
                        _ => unimplemented!(),
                    }),
                )
            }),
        }
    }
}

impl From<ast::Stmt<'_>> for syn::Stmt {
    fn from(stmt: ast::Stmt) -> Self {
        match stmt {
            ast::Stmt::ExprStmt(expr_stmt) => {
                syn::Stmt::Semi(expr_stmt.x.into(), <Token![;]>::default())
            }
            ast::Stmt::IfStmt(if_stmt) => syn::Stmt::Expr(syn::Expr::If(if_stmt.into())),
            _ => unimplemented!("{:?}", stmt),
        }
    }
}

impl From<token::Token> for syn::BinOp {
    fn from(token: token::Token) -> Self {
        use token::Token::*;
        match token {
            EQL => syn::BinOp::Eq(<Token![==]>::default()),
            REM => syn::BinOp::Rem(<Token![%]>::default()),
            _ => unimplemented!("{:?}", token),
        }
    }
}
