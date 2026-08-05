#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

fn main() {
    let _ = HashMap::<String, i64>::new();
    let _ = Arc::new((Mutex::new(0), Condvar::new()));
    let message = ::__gors_runtime::go_string_from_static(b"runtime-ok");
    ::__gors_runtime::print_go_string(message);
    ::__gors_runtime::print_newline();
}
