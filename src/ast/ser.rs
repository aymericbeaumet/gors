use crate::ast;
use std::io;

pub fn to_writer<W: io::Write>(w: &mut W, v: &ast::File) -> Result<(), Box<dyn std::error::Error>> {
    line(w, 0, 0)?;
    write!(w, "*ast.File {{\n")?;

    line(w, 1, 1)?;
    write!(w, "test\n")?;

    line(w, 1, 2)?;
    write!(w, "test\n")?;

    line(w, 1, 0)?;
    write!(w, "}}\n")?;

    Ok(())
}

fn line<W: io::Write>(w: &mut W, line: u32, depth: u32) -> io::Result<()> {
    write!(w, "{:>6}  ", line)?;
    for _ in 0..depth {
        write!(w, ".  ")?;
    }
    Ok(())
}
