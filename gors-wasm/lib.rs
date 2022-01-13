use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn build(input: String) -> String {
    console_error_panic_hook::set_once();

    let ast = gors::parser::parse_file("main.go", &input).unwrap();
    let compiled = gors::compiler::compile(ast).unwrap();

    let mut w = vec![];
    gors::codegen::fprint(&mut w, compiled).unwrap();

    String::from_utf8(w).unwrap()
}
