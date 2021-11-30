use crate::ast;
use codegen::Scope;

pub fn fprint<W: std::io::Write>(
    mut w: W,
    file: &ast::File,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut scope = Scope::new();

    scope
        .new_struct("Foo")
        .derive("Debug")
        .field("one", "usize")
        .field("two", "String");

    w.write_all(scope.to_string().as_bytes())?;

    Ok(())
}
