#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(buffer) = std::str::from_utf8(data) {
        let _ = gors::scanner::Scanner::new(file!(), buffer)
            .into_iter()
            .collect::<Vec<_>>();
    }
});
