use crate::diagnostics::print_error;
use crate::options::{Ast, Tokens};
use gors::error::Diagnostic;
use std::io::Write;

pub fn ast(cmd: Ast) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::with_capacity(8192, stdout.lock());

    match cmd.files.as_slice() {
        [file] => ast_single(file, &mut writer)?,
        files => {
            for file in files {
                write_file_result(&mut writer, file, ast_output(file))?;
            }
        }
    }
    writer.flush()?;

    Ok(())
}

fn ast_single(file: &str, writer: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    let parsed = match gors::parser::parse_file(file, &buffer) {
        Ok(parsed) => parsed,
        Err(err) => {
            let diagnostic = Diagnostic::from_parser_error(&err, file, &buffer);
            print_error(&diagnostic);
            std::process::exit(1);
        }
    };
    let (ast, _, _) = parsed.into_parts();
    gors::ast::fprint(writer, ast)?;
    Ok(())
}

fn ast_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
    let parsed = gors::parser::parse_file(file, &buffer).map_err(|error| error.to_string())?;
    let (ast, _, _) = parsed.into_parts();
    let mut output = Vec::new();
    gors::ast::fprint(&mut output, ast).map_err(|error| error.to_string())?;
    Ok(output)
}

pub fn tokens(cmd: Tokens) -> Result<(), Box<dyn std::error::Error>> {
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::with_capacity(8192, stdout.lock());

    match cmd.files.as_slice() {
        [file] => tokens_single(file, &mut writer)?,
        files => {
            for file in files {
                write_file_result(&mut writer, file, tokens_output(file))?;
            }
        }
    }
    writer.flush()?;

    Ok(())
}

fn tokens_single(file: &str, writer: &mut impl Write) -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read_to_string(file)?;
    for step in gors::scanner::Scanner::new(file, &buffer) {
        match step {
            Ok(token) => {
                serde_json::to_writer(&mut *writer, &token)?;
                writer.write_all(b"\n")?;
            }
            Err(err) => {
                let diagnostic = Diagnostic::from_scanner_error(&err, file, &buffer);
                print_error(&diagnostic);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn tokens_output(file: &str) -> Result<Vec<u8>, String> {
    let buffer = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
    let mut output = Vec::new();
    for step in gors::scanner::Scanner::new(file, &buffer) {
        let token = step.map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut output, &token).map_err(|error| error.to_string())?;
        output.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    Ok(output)
}

fn write_file_result(
    writer: &mut impl Write,
    path: &str,
    result: Result<Vec<u8>, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    match result {
        Ok(stdout) => serde_json::to_writer(
            &mut *writer,
            &serde_json::json!({
                "path": path,
                "ok": true,
                "stdout": String::from_utf8_lossy(&stdout),
            }),
        )?,
        Err(stderr) => serde_json::to_writer(
            &mut *writer,
            &serde_json::json!({
                "path": path,
                "ok": false,
                "stderr": stderr,
            }),
        )?,
    }
    writer.write_all(b"\n")?;
    Ok(())
}
