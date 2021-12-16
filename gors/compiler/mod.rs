#![allow(clippy::fallible_impl_from)] // TODO: switch to TryFrom

mod passes;

use crate::{ast, token};
use proc_macro2::Span;
use syn::Token;

pub fn compile(file: ast::File) -> Result<syn::File, Box<dyn std::error::Error>> {
    let mut out = file.into();
    passes::apply(&mut out);
    Ok(out)
}

impl From<ast::BasicLit<'_>> for syn::ExprLit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        Self {
            attrs: vec![],
            lit: basic_lit.into(),
        }
    }
}

impl From<ast::BasicLit<'_>> for syn::Lit {
    fn from(basic_lit: ast::BasicLit) -> Self {
        use token::Token::*;
        match basic_lit.kind {
            INT => Self::Int(syn::LitInt::new(basic_lit.value, Span::mixed_site())),
            STRING => {
                let mut value = basic_lit.value.chars();
                value.next();
                value.next_back();
                Self::Str(syn::LitStr::new(value.as_str(), Span::mixed_site()))
            }
            _ => unimplemented!("{:?}", basic_lit),
        }
    }
}

impl From<ast::BinaryExpr<'_>> for syn::ExprBinary {
    fn from(binary_expr: ast::BinaryExpr) -> Self {
        Self {
            attrs: vec![],
            left: Box::new((*binary_expr.x).into()),
            op: binary_expr.op.into(),
            right: Box::new((*binary_expr.y).into()),
        }
    }
}

impl From<ast::BlockStmt<'_>> for syn::Block {
    fn from(block_stmt: ast::BlockStmt) -> Self {
        Self {
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
        Self {
            attrs: vec![],
            label: None,
            block: block_stmt.into(),
        }
    }
}

impl From<ast::CallExpr<'_>> for syn::ExprCall {
    fn from(call_expr: ast::CallExpr) -> Self {
        let func = if let ast::Expr::Ident(ident) = *call_expr.fun {
            let mut segments = syn::punctuated::Punctuated::new();
            segments.push(syn::PathSegment {
                ident: ident.into(),
                arguments: syn::PathArguments::None,
            });

            syn::Expr::Path(syn::ExprPath {
                attrs: vec![],
                qself: None,
                path: syn::Path {
                    segments,
                    leading_colon: None,
                },
            })
        } else {
            (*call_expr.fun).into()
        };

        let mut args = syn::punctuated::Punctuated::new();
        if let Some(cargs) = call_expr.args {
            for arg in cargs {
                args.push(arg.into())
            }
        }

        Self {
            attrs: vec![],
            func: Box::new(func),
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
            ast::Expr::BasicLit(basic_lit) => Self::Lit(basic_lit.into()),
            ast::Expr::BinaryExpr(binary_expr) => Self::Binary(binary_expr.into()),
            ast::Expr::CallExpr(call_expr) => Self::Call(call_expr.into()),
            ast::Expr::Ident(ident) => Self::Path(ident.into()),
            ast::Expr::SelectorExpr(selector_expr) => Self::Path(selector_expr.into()),
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
                    ident: ident.into(),
                    arguments: syn::PathArguments::None,
                });
                Self::Path(syn::TypePath {
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

        let output = if let Some(results) = func_decl.type_.results {
            syn::ReturnType::Type(
                <Token![->]>::default(),
                Box::new(
                    results
                        .list
                        .into_iter()
                        .next()
                        .unwrap()
                        .type_
                        .unwrap()
                        .into(),
                ),
            )
        } else {
            syn::ReturnType::Default
        };

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
            output,
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
            Self::Public(syn::VisPublic {
                pub_token: <Token![pub]>::default(),
            })
        } else {
            Self::Inherited
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
        Self {
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
            ast::Stmt::AssignStmt(assign_stmt) => Self::Local(assign_stmt.into()),
            ast::Stmt::ExprStmt(expr_stmt) => {
                Self::Semi(expr_stmt.x.into(), <Token![;]>::default())
            }
            ast::Stmt::IfStmt(if_stmt) => Self::Expr(syn::Expr::If(if_stmt.into())),
            ast::Stmt::ReturnStmt(return_stmt) => Self::Expr(syn::Expr::Return(return_stmt.into())),
            _ => unimplemented!("{:?}", stmt),
        }
    }
}

// a := 1
// b, c := 2, 3
impl From<ast::AssignStmt<'_>> for syn::Local {
    fn from(assign_stmt: ast::AssignStmt) -> Self {
        if assign_stmt.lhs.len() != assign_stmt.rhs.len() {
            panic!("different numbers of lhs/rhs in assignment")
        }

        let pat = match assign_stmt.lhs.len() {
            0 => panic!("empty lhs"),
            1 => {
                if let ast::Expr::Ident(ident) = assign_stmt.lhs.into_iter().next().unwrap() {
                    syn::Pat::Ident(syn::PatIdent {
                        attrs: vec![],
                        ident: ident.into(),
                        by_ref: None,
                        subpat: None,
                        mutability: Some(<Token![mut]>::default()),
                    })
                } else {
                    panic!("expected ident")
                }
            }
            _ => {
                let mut elems = syn::punctuated::Punctuated::new();
                for expr in assign_stmt.lhs {
                    if let ast::Expr::Ident(ident) = expr {
                        elems.push(syn::Pat::Ident(syn::PatIdent {
                            attrs: vec![],
                            ident: ident.into(),
                            by_ref: None,
                            subpat: None,
                            mutability: Some(<Token![mut]>::default()),
                        }))
                    } else {
                        panic!("expecting ident")
                    }
                }
                syn::Pat::Tuple(syn::PatTuple {
                    attrs: vec![],
                    paren_token: syn::token::Paren {
                        ..Default::default()
                    },
                    elems,
                })
            }
        };

        let init = match assign_stmt.rhs.len() {
            0 => panic!("empty rhs"),
            1 => assign_stmt.rhs.into_iter().next().unwrap().into(),
            _ => {
                let mut elems = syn::punctuated::Punctuated::new();
                for expr in assign_stmt.rhs {
                    elems.push(expr.into())
                }
                syn::Expr::Tuple(syn::ExprTuple {
                    attrs: vec![],
                    elems,
                    paren_token: syn::token::Paren {
                        ..Default::default()
                    },
                })
            }
        };

        Self {
            attrs: vec![],
            pat,
            init: Some((<Token![=]>::default(), Box::new(init))),
            let_token: <Token![let]>::default(),
            semi_token: <Token![;]>::default(),
        }
    }
}

impl From<ast::ReturnStmt<'_>> for syn::ExprReturn {
    fn from(return_stmt: ast::ReturnStmt) -> Self {
        let expr: syn::Expr = return_stmt.results.into_iter().next().unwrap().into();
        Self {
            attrs: vec![],
            expr: Some(Box::new(expr)),
            return_token: <Token![return]>::default(),
        }
    }
}

impl From<token::Token> for syn::BinOp {
    fn from(token: token::Token) -> Self {
        use token::Token::*;
        match token {
            ADD => Self::Add(<Token![+]>::default()),
            SUB => Self::Sub(<Token![-]>::default()),
            MUL => Self::Mul(<Token![*]>::default()),
            QUO => Self::Div(<Token![/]>::default()),
            REM => Self::Rem(<Token![%]>::default()),
            LAND => Self::And(<Token![&&]>::default()),
            LOR => Self::Or(<Token![||]>::default()),
            XOR => Self::BitXor(<Token![^]>::default()),
            AND => Self::BitAnd(<Token![&]>::default()),
            OR => Self::BitOr(<Token![|]>::default()),
            SHL => Self::Shl(<Token![<<]>::default()),
            SHR => Self::Shr(<Token![>>]>::default()),
            EQL => Self::Eq(<Token![==]>::default()),
            LSS => Self::Lt(<Token![<]>::default()),
            LEQ => Self::Le(<Token![<=]>::default()),
            NEQ => Self::Ne(<Token![!=]>::default()),
            GEQ => Self::Ge(<Token![>=]>::default()),
            GTR => Self::Gt(<Token![>]>::default()),
            _ => unreachable!("unsupported binary op: {:?}", token),
        }
    }
}
