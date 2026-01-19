#![warn(clippy::all, clippy::nursery)]
// Allow some lints that are overly pedantic for this codebase
#![allow(clippy::collapsible_if)] // Let chains are suggested but not always clearer
#![allow(clippy::option_if_let_else)] // map_or_else is not always clearer
#![allow(clippy::use_self)] // Explicit type names can improve readability
#![allow(clippy::missing_const_for_fn)] // Not all functions need to be const
#![allow(clippy::unnecessary_struct_initialization)] // Sometimes explicit initialization is clearer
#![allow(clippy::needless_pass_by_ref_mut)] // Sometimes &mut is needed for consistency
#![allow(dead_code)] // Some code is intentionally unused for future features

pub mod ast;
pub mod codegen;
pub mod compiler;
pub mod error;
pub mod parser;
pub mod scanner;
pub mod token;
