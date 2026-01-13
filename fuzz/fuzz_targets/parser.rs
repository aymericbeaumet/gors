#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Try to parse the input as a Go source file
        let _ = gors::parser::parse_file("fuzz.go", s);
    }
});
