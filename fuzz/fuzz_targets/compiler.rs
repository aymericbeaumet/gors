#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(buffer) = std::str::from_utf8(data) {
        if let Ok(ast) = gors::parser::parse_file(file!(), buffer) {
            _ = gors::compiler::compile(ast);
        }
    }
});
