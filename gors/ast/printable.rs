use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use crate::token;
use std::collections::BTreeMap;
use std::io::Write;

impl<W: Write, T: Printable<W>> Printable<W> for Box<T> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        (**self).print(p)?;
        Ok(())
    }
}

impl<W: Write, T: Printable<W>> Printable<W> for Option<T> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if let Some(node) = self {
            node.print(p)?;
        } else {
            p.write("nil")?;
            p.newline()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for BTreeMap<&str, ast::Object<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("map[string]*ast.Object (len = 0) {}")?;
            p.newline()?;
        } else {
            write!(p.w, "map[string]*ast.Object (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (key, value) in self.iter() {
                p.prefix()?;
                write!(p.w, "{:?}: ", key)?;
                value.print(p)?;
            }
            p.close_bracket()?;
        }

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::CommentGroup> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.CommentGroup (len = {}) ", self.len())?;
            p.open_bracket()?;
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Field<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]*ast.Field (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, decl) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                decl.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Decl<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Decl (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, decl) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                decl.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Spec<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Spec (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, spec) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                spec.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::FieldList<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FieldList ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Opening: ")?;
        if let Some(opening) = self.opening {
            opening.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("List: ")?;
        self.list.print(p)?;

        p.prefix()?;
        p.write("Closing: ")?;
        if let Some(closing) = self.closing {
            closing.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Ident<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]*ast.Ident (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, ident) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                ident.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ReturnStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ReturnStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Return: ")?;
        self.return_.print(p)?;

        p.prefix()?;
        p.write("Results: ")?;
        self.results.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BinaryExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BinaryExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("OpPos: ")?;
        self.op_pos.print(p)?;

        p.prefix()?;
        p.write("Op: ")?;
        self.op.print(p)?;

        p.prefix()?;
        p.write("Y: ")?;
        self.y.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BasicLit<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BasicLit ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("ValuePos: ")?;
        self.value_pos.print(p)?;

        p.prefix()?;
        p.write("Kind: ")?;
        self.kind.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        write!(p.w, "{:?}", self.value)?;
        p.newline()?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<&ast::ImportSpec<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]*ast.ImportSpec (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, spec) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                spec.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Ellipsis<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Ellipsis ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Ellipsis: ")?;
        self.ellipsis.print(p)?;

        p.prefix()?;
        p.write("Elt: ")?;
        self.elt.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CommentGroup {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Field<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Field ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Names: ")?;
        self.names.print(p)?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Tag: ")?;
        self.tag.print(p)?;

        p.prefix()?;
        p.write("Comment: ")?;
        self.comment.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Stmt<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Stmt (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, stmt) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                stmt.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::AssignStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.AssignStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lhs: ")?;
        self.lhs.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.prefix()?;
        p.write("Rhs: ")?;
        self.rhs.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BlockStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BlockStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lbrace: ")?;
        self.lbrace.print(p)?;

        p.prefix()?;
        p.write("List: ")?;
        self.list.print(p)?;

        p.prefix()?;
        p.write("Rbrace: ")?;
        self.rbrace.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::FuncType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FuncType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Func: ")?;
        if let Some(func) = self.func {
            func.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Params: ")?;
        self.params.print(p)?;

        p.prefix()?;
        p.write("Results: ")?;
        self.results.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::GenDecl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.GenDecl ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.prefix()?;
        p.write("Lparen: ")?;
        if let Some(lparen) = self.lparen {
            lparen.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Specs: ")?;
        self.specs.print(p)?;

        p.prefix()?;
        p.write("Rparen: ")?;
        if let Some(rparen) = self.rparen {
            rparen.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::FuncDecl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.prevent_circular(self)? {
            return Ok(());
        }

        p.write("*ast.FuncDecl ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Recv: ")?;
        self.recv.print(p)?;

        p.prefix()?;
        p.write("Name: ")?;
        self.name.print(p)?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::File<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.prefix()?;
        p.write("*ast.File ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Package: ")?;
        self.package.print(p)?;

        p.prefix()?;
        p.write("Name: ")?;
        self.name.print(p)?;

        p.prefix()?;
        p.write("Decls: ")?;
        self.decls.print(p)?;

        p.prefix()?;
        p.write("Scope: ")?;
        self.scope.print(p)?;

        p.prefix()?;
        p.write("Imports: ")?;
        self.imports().print(p)?;

        p.prefix()?;
        p.write("Unresolved: ")?;
        self.unresolved.print(p)?;

        p.prefix()?;
        p.write("Comments: ")?;
        self.comments.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Ident<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.prevent_circular(self)? {
            return Ok(());
        }

        p.write("*ast.Ident ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("NamePos: ")?;
        self.name_pos.print(p)?;

        p.prefix()?;
        write!(p.w, "Name: {:?}", self.name)?;
        p.newline()?;

        p.prefix()?;
        p.write("Obj: ")?;
        self.obj.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for () {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ImportSpec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.prevent_circular(self)? {
            return Ok(());
        }

        p.write("*ast.ImportSpec ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Name: ")?;
        self.name.print(p)?;

        p.prefix()?;
        p.write("Path: ")?;
        self.path.print(p)?;

        p.prefix()?;
        p.write("Comment: ")?;
        self.comment.print(p)?;

        p.prefix()?;
        p.write("EndPos: -")?;
        p.newline()?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::StructType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.StructType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Struct: ")?;
        self.struct_.print(p)?;

        p.prefix()?;
        p.write("Fields: ")?;
        self.fields.print(p)?;

        p.prefix()?;
        p.write("Incomplete: ")?;
        self.incomplete.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::TypeSpec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.TypeSpec ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Name: ")?;
        self.name.print(p)?;

        p.prefix()?;
        p.write("Assign: ")?;
        if let Some(assign) = self.assign {
            assign.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Comment: ")?;
        self.comment.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::InterfaceType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.InterfaceType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Interface: ")?;
        self.interface.print(p)?;

        p.prefix()?;
        p.write("Methods: ")?;
        self.methods.print(p)?;

        p.prefix()?;
        p.write("Incomplete: ")?;
        self.incomplete.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::StarExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.StarExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Star: ")?;
        self.star.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::UnaryExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.UnaryExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("OpPos: ")?;
        self.op_pos.print(p)?;

        p.prefix()?;
        p.write("Op: ")?;
        self.op.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CallExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.CallExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Fun: ")?;
        self.fun.print(p)?;

        p.prefix()?;
        p.write("Lparen: ")?;
        self.lparen.print(p)?;

        p.prefix()?;
        p.write("Args: ")?;
        self.args.print(p)?;

        p.prefix()?;
        p.write("Ellipsis: ")?;
        if let Some(ellipsis) = self.ellipsis {
            ellipsis.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Rparen: ")?;
        self.rparen.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ParenExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ParenExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lparen: ")?;
        self.lparen.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Rparen: ")?;
        self.rparen.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SelectorExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SelectorExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Sel: ")?;
        self.sel.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ExprStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ExprStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IfStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IfStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("If: ")?;
        self.if_.print(p)?;

        p.prefix()?;
        p.write("Init: ")?;
        self.init.print(p)?;

        p.prefix()?;
        p.write("Cond: ")?;
        self.cond.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.prefix()?;
        p.write("Else: ")?;
        self.else_.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IncDecStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IncDecStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ChanType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ChanType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Begin: ")?;
        self.begin.print(p)?;

        p.prefix()?;
        p.write("Arrow: ")?;
        if let Some(arrow) = self.arrow {
            arrow.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Dir: ")?;
        self.dir.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::GoStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.GoStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Go: ")?;
        self.go.print(p)?;

        p.prefix()?;
        p.write("Call: ")?;
        self.call.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::FuncLit<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FuncLit ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SendStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SendStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Chan: ")?;
        self.chan.print(p)?;

        p.prefix()?;
        p.write("Arrow: ")?;
        self.arrow.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::DeclStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.DeclStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Decl: ")?;
        self.decl.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ValueSpec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.prevent_circular(self)? {
            return Ok(());
        }

        p.write("*ast.ValueSpec ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Doc: ")?;
        self.doc.print(p)?;

        p.prefix()?;
        p.write("Names: ")?;
        self.names.print(p)?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Values: ")?;
        self.values.print(p)?;

        p.prefix()?;
        p.write("Comment: ")?;
        self.comment.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Expr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Expr::BasicLit(node) => node.print(p),
            ast::Expr::BinaryExpr(node) => node.print(p),
            ast::Expr::CallExpr(node) => node.print(p),
            ast::Expr::ChanType(node) => node.print(p),
            ast::Expr::Ellipsis(node) => node.print(p),
            ast::Expr::FuncLit(node) => node.print(p),
            ast::Expr::FuncType(node) => node.print(p),
            ast::Expr::Ident(node) => node.print(p),
            ast::Expr::InterfaceType(node) => node.print(p),
            ast::Expr::ParenExpr(node) => node.print(p),
            ast::Expr::SelectorExpr(node) => node.print(p),
            ast::Expr::StarExpr(node) => node.print(p),
            ast::Expr::StructType(node) => node.print(p),
            ast::Expr::UnaryExpr(node) => node.print(p),
        }
    }
}

impl<W: Write> Printable<W> for Vec<ast::Expr<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Expr (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, expr) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                expr.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Object<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.prevent_circular(self)? {
            return Ok(());
        }

        p.write("*ast.Object ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Kind: ")?;
        self.kind.print(p)?;

        p.prefix()?;
        write!(p.w, "Name: {:?}", self.name)?;
        p.newline()?;

        p.prefix()?;
        p.write("Decl: ")?;
        self.decl.print(p)?;

        p.prefix()?;
        p.write("Data: ")?;
        self.data.print(p)?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Scope<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Scope ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Outer: ")?;
        self.outer.print(p)?;

        p.prefix()?;
        p.write("Objects: ")?;
        self.objects.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Spec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Spec::ImportSpec(spec) => spec.print(p),
            ast::Spec::TypeSpec(spec) => spec.print(p),
            ast::Spec::ValueSpec(spec) => spec.print(p),
        }
    }
}

impl<W: Write> Printable<W> for ast::Decl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Decl::FuncDecl(decl) => decl.print(p),
            ast::Decl::GenDecl(decl) => decl.print(p),
        }
    }
}

impl<W: Write> Printable<W> for ast::ObjDecl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::ObjDecl::FuncDecl(decl) => decl.print(p),
            ast::ObjDecl::ValueSpec(decl) => decl.print(p),
        }
    }
}

impl<W: Write> Printable<W> for ast::Stmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Stmt::AssignStmt(stmt) => stmt.print(p),
            ast::Stmt::BlockStmt(stmt) => stmt.print(p),
            ast::Stmt::DeclStmt(stmt) => stmt.print(p),
            ast::Stmt::ExprStmt(stmt) => stmt.print(p),
            ast::Stmt::GoStmt(stmt) => stmt.print(p),
            ast::Stmt::IfStmt(stmt) => stmt.print(p),
            ast::Stmt::IncDecStmt(stmt) => stmt.print(p),
            ast::Stmt::ReturnStmt(stmt) => stmt.print(p),
            ast::Stmt::SendStmt(stmt) => stmt.print(p),
        }
    }
}

impl<W: Write> Printable<W> for ast::ObjKind {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::ObjKind::Con => p.write("const")?,
            ast::ObjKind::Fun => p.write("func")?,
            ast::ObjKind::Var => p.write("var")?,
        }
        p.newline()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for token::Position<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(
            p.w,
            "{}/{}:{}:{}",
            self.directory, self.file, self.line, self.column,
        )?;
        p.newline()?;
        Ok(())
    }
}

impl<W: Write> Printable<W> for token::Token {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write(self.into())?;
        p.newline()?;
        Ok(())
    }
}

impl<W: Write> Printable<W> for bool {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(p.w, "{}", self)?;
        p.newline()?;
        Ok(())
    }
}

impl<W: Write> Printable<W> for usize {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(p.w, "{}", self)?;
        p.newline()?;
        Ok(())
    }
}

impl<W: Write> Printable<W> for u8 {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(p.w, "{}", self)?;
        p.newline()?;
        Ok(())
    }
}
