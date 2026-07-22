use super::print_go_string;
use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use std::collections::BTreeMap;
use std::io::Write;

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

impl<W: Write> Printable<W> for Vec<ast::CommentGroup<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]*ast.CommentGroup (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, comment_group) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                comment_group.print(p)?;
            }
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

impl<W: Write> Printable<W> for ast::CommentGroup<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        // Check if this comment group was already printed (for back-references)
        if p.try_print_line(self)? {
            return Ok(());
        }

        if self.list.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "*ast.CommentGroup ")?;
            p.open_bracket()?;
            p.prefix()?;
            write!(p.w, "List: []*ast.Comment (len = {}) ", self.list.len())?;
            p.open_bracket()?;
            for (i, comment) in self.list.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: *ast.Comment ", i)?;
                p.open_bracket()?;
                p.prefix()?;
                write!(p.w, "Slash: {}", comment.slash)?;
                p.newline()?;
                p.prefix()?;
                write!(p.w, "Text: ")?;
                print_go_string(&mut p.w, comment.text)?;
                p.newline()?;
                p.close_bracket()?;
            }
            p.close_bracket()?;
            p.close_bracket()?;
        }
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
        p.write("TypeParams: ")?;
        self.type_params.print(p)?;

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
        p.write("FileStart: ")?;
        self.file_start.print(p)?;

        p.prefix()?;
        p.write("FileEnd: ")?;
        self.file_end.print(p)?;

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

        p.prefix()?;
        p.write("GoVersion: ")?;
        write!(p.w, "{:?}", self.go_version)?;
        p.newline()?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ImportSpec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if p.try_print_line(self)? {
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
        p.write("TypeParams: ")?;
        self.type_params.print(p)?;

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

impl<W: Write> Printable<W> for ast::ValueSpec<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
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

impl<W: Write> Printable<W> for ast::Object<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
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
