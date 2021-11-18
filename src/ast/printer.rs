use crate::ast;
use crate::token;
use std::collections::HashMap;
use std::hash::Hasher;
use std::io::Write;

pub struct Printer<W: Write> {
    w: W,
    line: usize,
    depth: usize,
    lines: HashMap<u64, usize>,
}

impl<W: Write> Printer<W> {
    pub fn new(w: W) -> Self {
        Printer {
            w,
            line: 0,
            depth: 0,
            lines: HashMap::default(),
        }
    }

    pub fn print(&mut self, file: &ast::File) -> Result<(), Box<dyn std::error::Error>> {
        self.reset();
        file.print(self)?;
        self.w.flush()?;
        Ok(())
    }

    fn prefix(&mut self) -> std::io::Result<()> {
        write!(self.w, "{:6}  ", self.line)?;
        for _ in 0..self.depth {
            self.write(".  ")?;
        }
        Ok(())
    }

    fn open_bracket(&mut self) -> std::io::Result<()> {
        self.depth += 1;
        self.write("{")?;
        self.newline()
    }

    fn close_bracket(&mut self) -> std::io::Result<()> {
        self.depth -= 1;
        self.prefix()?;
        self.write("}")?;
        self.newline()
    }

    fn newline(&mut self) -> std::io::Result<()> {
        self.line += 1;
        self.write("\n")
    }

    fn write(&mut self, buf: &str) -> std::io::Result<()> {
        self.w.write_all(buf.as_bytes())
    }

    fn reset(&mut self) {
        self.line = 0;
        self.depth = 0;
    }

    fn set_line<T>(&mut self, val: &T) {
        self.lines.insert(hash(val), self.line);
    }

    fn get_line<T>(&self, val: &T) -> usize {
        self.lines.get(&hash(val)).copied().unwrap_or(0)
    }
}

fn hash<T>(val: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::ptr::hash(val, &mut hasher);
    hasher.finish()
}

type PrintResult = Result<(), Box<dyn std::error::Error>>;

trait Printable<W: Write> {
    fn print(&self, _: &mut Printer<W>) -> PrintResult;
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

impl<W: Write> Printable<W> for HashMap<&str, &ast::Object<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("map[string]*ast.Object (len = 0) {}")?;
            p.newline()?;
        } else {
            write!(p.w, "map[string]*ast.Object (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (key, value) in self.iter() {
                p.prefix()?;
                write!(p.w, "{:?}: *(obj @ {})", key, p.get_line(*value))?;
                p.newline()?;
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

impl<W: Write> Printable<W> for Vec<&ast::Decl<'_>> {
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
            write!(p.w, "[]ast.Ident (len = {}) ", self.len())?;
            p.open_bracket()?;
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<&ast::ImportSpec> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.ImportSpec (len = {}) ", self.len())?;
            p.open_bracket()?;
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::CommentGroup {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::Field {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<&ast::Stmt> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
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
        p.write("List: nil")?;
        p.newline()?;

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

impl<W: Write> Printable<W> for &ast::FuncDecl<'_> {
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
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ImportSpec {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for &ast::Object<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.set_line(*self);

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

impl<W: Write> Printable<W> for ast::Decl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            &ast::Decl::FuncDecl(decl) => {
                p.set_line(decl);

                p.write("*ast.FuncDecl ")?;
                p.open_bracket()?;

                p.prefix()?;
                p.write("Doc: ")?;
                decl.doc.print(p)?;

                p.prefix()?;
                p.write("Recv: ")?;
                decl.recv.print(p)?;

                p.prefix()?;
                p.write("Name: ")?;
                decl.name.print(p)?;

                p.prefix()?;
                p.write("Type: ")?;
                decl.type_.print(p)?;

                p.prefix()?;
                p.write("Body: ")?;
                decl.body.print(p)?;

                p.close_bracket()?;
            }
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ObjKind {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            &ast::ObjKind::Fun => p.write("func")?,
        }
        p.newline()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ObjDecl<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        let line = match self {
            &ast::ObjDecl::FuncDecl(decl) => p.get_line(decl),
        };
        write!(p.w, "*(obj @ {})", line)?;
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
