use std::fmt;

#[derive(Debug)]
pub enum CodegenError {
    Rustfmt(String),
}

impl std::error::Error for CodegenError {}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "codegen error: {:?}", self)
    }
}

pub fn fprint<W: std::io::Write, R: Fn(&str) -> String>(
    mut w: W,
    file: syn::File,
    rustfmt: R,
) -> Result<(), Box<dyn std::error::Error>> {
    let ugly = (quote::quote! { #file }).to_string();
    let pretty = rustfmt(&ugly);

    for (i, line) in pretty.lines().enumerate() {
        if i > 0 && (line.starts_with("fn") || line.starts_with("pub fn")) {
            w.write_all(b"\n")?;
        }
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
    }

    Ok(())
}
