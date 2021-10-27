use crate::ast;
use std::io;

struct Serializer<W> {
    writer: W,
    line: u32,
    depth: u32,
}

enum Suffix {
    OpenBracket,
    Newline,
    CloseBracket,
}

impl<W: io::Write> Serializer<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            line: 0,
            depth: 0,
        }
    }

    fn serialize_file(&mut self, value: &ast::File) -> io::Result<()> {
        self.prefix()?;
        write!(self.writer, "*ast.File")?;
        self.suffix(Suffix::OpenBracket)?;

        self.prefix()?;
        write!(self.writer, "Package:")?;
        self.suffix(Suffix::Newline)?;

        self.prefix()?;
        write!(self.writer, "Name:")?;
        self.suffix(Suffix::Newline)?;

        self.prefix()?;
        write!(
            self.writer,
            "Decls: []ast.Decl (len = {})",
            value.decls.len()
        )?;
        self.suffix(Suffix::OpenBracket)?;

        self.suffix(Suffix::CloseBracket)?;

        self.suffix(Suffix::CloseBracket)?;

        self.writer.flush()
    }

    fn prefix(&mut self) -> io::Result<()> {
        write!(self.writer, "{:>6}  ", self.line)?;
        for _ in 0..self.depth {
            write!(self.writer, ".  ")?;
        }
        self.line += 1;
        Ok(())
    }

    fn suffix(&mut self, s: Suffix) -> io::Result<()> {
        match s {
            Suffix::OpenBracket => {
                write!(self.writer, " {{\n")?;
                self.depth += 1;
            }
            Suffix::Newline => {
                write!(self.writer, "\n")?;
            }
            Suffix::CloseBracket => {
                self.depth -= 1;
                self.prefix()?;
                write!(self.writer, "}}\n")?;
            }
        }
        Ok(())
    }
}

pub fn to_writer<W: io::Write>(writer: W, value: &ast::File) -> io::Result<()> {
    let mut ser = Serializer::new(writer);
    ser.serialize_file(value)
}
