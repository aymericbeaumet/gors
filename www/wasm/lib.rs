mod build_result;
mod comments;
mod compiler;

pub use build_result::BuildResult;
pub use compiler::GorsCompiler;

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests;
