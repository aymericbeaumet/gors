use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use crate::token;
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

impl<W: Write> Printable<W> for () {
    fn print(&self, _: &mut Printer<W>) -> PrintResult {
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ObjKind {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            Self::Con => p.write("const")?,
            Self::Fun => p.write("func")?,
            Self::Var => p.write("var")?,
        }
        p.newline()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for token::Position<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.file.is_empty() {
            write!(p.w, "{}", self.line)?;
        } else if self.file.starts_with('/') {
            write!(p.w, "{}:{}", self.file, self.line)?;
        } else {
            write!(p.w, "{}/{}:{}", self.directory, self.file, self.line)?;
        }
        if self.column != 0 {
            write!(p.w, ":{}", self.column)?;
        }
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
