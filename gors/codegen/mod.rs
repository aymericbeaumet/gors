use std::fmt;
use std::{
    io::Write,
    process::{Command, Stdio},
};

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

pub fn fprint<W: std::io::Write>(
    mut w: W,
    file: syn::File,
    rustfmt: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let ugly = (quote::quote! { #file }).to_string();
    if !rustfmt {
        w.write_all(ugly.as_bytes())?;
        return Ok(());
    }

    let mut cmd = Command::new("rustfmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = cmd.stdin.as_mut().unwrap();
    stdin.write_all(ugly.as_bytes())?;

    let output = cmd.wait_with_output()?;
    if !output.status.success() {
        return Err(Box::new(CodegenError::Rustfmt(
            String::from_utf8(output.stderr).unwrap(),
        )));
    }

    w.write_all(&output.stdout)?;

    Ok(())
}
