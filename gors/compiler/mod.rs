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
            stmts: block_stmt.list.into_iter().flat_map(Vec::from).collect(),
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

impl From<ast::Stmt<'_>> for Vec<syn::Stmt> {
    fn from(stmt: ast::Stmt) -> Self {
        let semi = <Token![;]>::default();
        match stmt {
            ast::Stmt::AssignStmt(s) => s.into(),
            ast::Stmt::DeclStmt(s) => s.into(),
            ast::Stmt::ExprStmt(s) => vec![syn::Stmt::Semi(s.x.into(), semi)],
            ast::Stmt::ForStmt(s) => vec![syn::Stmt::Expr(s.into())],
            ast::Stmt::IfStmt(s) => vec![syn::Stmt::Expr(syn::Expr::If(s.into()))],
            ast::Stmt::IncDecStmt(s) => vec![syn::Stmt::Semi(syn::Expr::AssignOp(s.into()), semi)],
            ast::Stmt::ReturnStmt(s) => vec![syn::Stmt::Expr(syn::Expr::Return(s.into()))],
            _ => unimplemented!("{:?}", stmt),
        }
    }
}

impl From<ast::IncDecStmt<'_>> for syn::ExprAssignOp {
    fn from(inc_dec_stmt: ast::IncDecStmt) -> Self {
        Self {
            attrs: vec![],
            op: match inc_dec_stmt.tok {
                token::Token::INC => syn::BinOp::AddEq(<Token![+=]>::default()),
                token::Token::DEC => syn::BinOp::SubEq(<Token![-=]>::default()),
                _ => unreachable!("implementation error"),
            },
            left: Box::new(inc_dec_stmt.x.into()),
            right: Box::new(syn::Expr::Lit(syn::ExprLit {
                attrs: vec![],
                lit: syn::Lit::Int(syn::LitInt::new("1", Span::mixed_site())),
            })),
        }
    }
}

impl From<ast::ForStmt<'_>> for syn::Expr {
    fn from(for_stmt: ast::ForStmt) -> Self {
        let mut stmts = vec![];

        if let Some(init) = for_stmt.init {
            stmts.extend(Vec::from(*init));
        }

        let mut body: syn::Block = for_stmt.body.into();
        if let Some(post) = for_stmt.post {
            body.stmts.extend(Vec::from(*post));
        }

        stmts.push(syn::Stmt::Expr(if let Some(cond) = for_stmt.cond {
            Self::While(syn::ExprWhile {
                attrs: vec![],
                label: None,
                cond: Box::new(cond.into()),
                body,
                while_token: <Token![while]>::default(),
            })
        } else {
            Self::Loop(syn::ExprLoop {
                attrs: vec![],
                label: None,
                body,
                loop_token: <Token![loop]>::default(),
            })
        }));

        Self::Block(syn::ExprBlock {
            attrs: vec![],
            label: None,
            block: syn::Block {
                stmts,
                brace_token: syn::token::Brace {
                    ..Default::default()
                },
            },
        })
    }
}

impl From<ast::DeclStmt<'_>> for Vec<syn::Stmt> {
    fn from(decl_stmt: ast::DeclStmt) -> Self {
        // TODO
        vec![]
    }
}

impl From<ast::AssignStmt<'_>> for Vec<syn::Stmt> {
    fn from(assign_stmt: ast::AssignStmt) -> Self {
        if assign_stmt.lhs.len() != assign_stmt.rhs.len() {
            panic!("different numbers of lhs/rhs in assignment")
        }

        if assign_stmt.lhs.is_empty() {
            panic!("empty lhs")
        }

        // a := 1
        // b, c := 2, 3
        if assign_stmt.tok == token::Token::DEFINE {
            let pat = match assign_stmt.lhs.len() {
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

            return vec![syn::Stmt::Local(syn::Local {
                attrs: vec![],
                pat,
                init: Some((<Token![=]>::default(), Box::new(init))),
                let_token: <Token![let]>::default(),
                semi_token: <Token![;]>::default(),
            })];
        }

        // a = 1
        // b, c = 2, 3
        if assign_stmt.tok == token::Token::ASSIGN {
            if assign_stmt.lhs.len() == 1 {
                return vec![syn::Stmt::Semi(
                    syn::Expr::Assign(syn::ExprAssign {
                        attrs: vec![],
                        left: Box::new(assign_stmt.lhs.into_iter().next().unwrap().into()),
                        eq_token: <Token![=]>::default(),
                        right: Box::new(assign_stmt.rhs.into_iter().next().unwrap().into()),
                    }),
                    <Token![;]>::default(),
                )];
            }

            let mut out = vec![];

            for (lhs, rhs) in assign_stmt.lhs.iter().zip(assign_stmt.rhs.into_iter()) {
                if let ast::Expr::Ident(ident) = lhs {
                    out.push(syn::Stmt::Local(syn::Local {
                        attrs: vec![],
                        semi_token: <Token![;]>::default(),
                        let_token: <Token![let]>::default(),
                        pat: syn::Pat::Ident(syn::PatIdent {
                            attrs: vec![],
                            mutability: None,
                            subpat: None,
                            by_ref: None,
                            ident: syn::Ident::new(
                                &format!("{}__", ident.name),
                                Span::mixed_site(),
                            ),
                        }),
                        init: Some((<Token![=]>::default(), Box::new(rhs.into()))),
                    }))
                } else {
                    panic!("expecting ident")
                }
            }

            for lhs in assign_stmt.lhs {
                let mut segments = syn::punctuated::Punctuated::new();
                if let ast::Expr::Ident(ident) = &lhs {
                    segments.push(syn::PathSegment {
                        ident: syn::Ident::new(&format!("{}__", ident.name), Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    });

                    out.push(syn::Stmt::Semi(
                        syn::Expr::Assign(syn::ExprAssign {
                            attrs: vec![],
                            left: Box::new(lhs.into()),
                            eq_token: <Token![=]>::default(),
                            right: Box::new(syn::Expr::Path(syn::ExprPath {
                                attrs: vec![],
                                path: syn::Path {
                                    leading_colon: None,
                                    segments,
                                },
                                qself: None,
                            })),
                        }),
                        <Token![;]>::default(),
                    ))
                } else {
                    panic!("expecting ident")
                }
            }

            return out;
        }

        // e += 4
        if assign_stmt.tok.is_assign_op() {
            if assign_stmt.lhs.len() != 1 {
                panic!("only supports a single lhs element")
            }
            return vec![syn::Stmt::Semi(
                syn::Expr::Assign(syn::ExprAssign {
                    attrs: vec![],
                    left: Box::new(assign_stmt.lhs.into_iter().next().unwrap().into()),
                    eq_token: <Token![=]>::default(),
                    right: Box::new(assign_stmt.rhs.into_iter().next().unwrap().into()),
                }),
                <Token![;]>::default(),
            )];
        }

        unimplemented!(
            "implementation error, unexpected token {:?}",
            assign_stmt.tok
        )
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
