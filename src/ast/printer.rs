use crate::ast;
use crate::token;
use std::io::Write;

pub struct Printer<W: Write> {
    w: W,
    line: usize,
    depth: usize,
}

impl<W: Write> Printer<W> {
    pub fn new(w: W) -> Self {
        Printer {
            w,
            line: 0,
            depth: 0,
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
        self.write(" {")?;
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
}

type PrintResult = Result<(), Box<dyn std::error::Error>>;

trait Printable<W: Write> {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
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

impl<W: Write> Printable<W> for ast::CommentGroup {}

impl<W: Write> Printable<W> for ast::File<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.prefix()?;
        p.write("*ast.File")?;
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

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Ident<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Ident")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("NamePos: ")?;
        self.name_pos.print(p)?;

        p.prefix()?;
        p.write("Name: ")?;
        self.name.print(p)?;

        p.prefix()?;
        p.write("Obj: ")?;
        self.obj.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Object {}

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

impl<W: Write> Printable<W> for &str {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        write!(p.w, "{:?}", self)?;
        p.newline()?;
        Ok(())
    }
}
