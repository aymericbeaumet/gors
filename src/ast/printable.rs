use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use crate::token;
use std::collections::BTreeMap;
use std::io::Write;

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

impl<W: Write> Printable<W> for BTreeMap<&str, &ast::Object<'_>> {
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

impl<W: Write> Printable<W> for Vec<&ast::CommentGroup> {
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

impl<W: Write> Printable<W> for Vec<&ast::Field> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Field (len = {}) ", self.len())?;
            p.open_bracket()?;
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

impl<W: Write> Printable<W> for &ast::FieldList<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FieldList ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Opening: ")?;
        self.opening.print(p)?;

        p.prefix()?;
        p.write("List: ")?;
        self.list.print(p)?;

        p.prefix()?;
        p.write("Closing: ")?;
        self.closing.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<&ast::Ident<'_>> {
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

impl<W: Write> Printable<W> for &ast::ReturnStmt<'_> {
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

impl<W: Write> Printable<W> for &ast::BinaryExpr<'_> {
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

impl<W: Write> Printable<W> for &ast::BasicLit<'_> {
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

impl<W: Write> Printable<W> for &ast::CommentGroup {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::Field {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
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

impl<W: Write> Printable<W> for &ast::AssignStmt<'_> {
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

impl<W: Write> Printable<W> for &ast::BlockStmt<'_> {
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

impl<W: Write> Printable<W> for &ast::FuncType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FuncType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Func: ")?;
        self.func.print(p)?;

        p.prefix()?;
        p.write("Params: ")?;
        self.params.print(p)?;

        p.prefix()?;
        p.write("Results: nil")?;
        p.newline()?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::GenDecl<'_> {
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

impl<W: Write> Printable<W> for &ast::FuncDecl<'_> {
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

impl<W: Write> Printable<W> for &ast::File<'_> {
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
        self.imports.print(p)?;

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

impl<W: Write> Printable<W> for &ast::Ident<'_> {
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
        self.obj.get().print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for () {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::ImportSpec<'_> {
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

impl<W: Write> Printable<W> for &ast::ValueSpec<'_> {
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
            ast::Expr::Ident(node) => node.print(p),
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

impl<W: Write> Printable<W> for &ast::Object<'_> {
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

impl<W: Write> Printable<W> for &ast::Scope<'_> {
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
            ast::Stmt::ReturnStmt(stmt) => stmt.print(p),
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

impl<W: Write> Printable<W> for usize {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(p.w, "{}", self)?;
        p.newline()?;
        Ok(())
    }
}
