//! Canonical runtime call plans selected before terminal syntax emission.

use super::{PrintStep, RustType};

pub(in crate::compiler) fn print_plan(types: &[RustType], newline: bool) -> Option<Vec<PrintStep>> {
    if types.is_empty() {
        return Some(vec![if newline {
            PrintStep::PrintNewline
        } else {
            PrintStep::PrintEmpty
        }]);
    }

    let mut steps = Vec::with_capacity(types.len().saturating_mul(2));
    for (argument, ty) in types.iter().enumerate() {
        let last = argument + 1 == types.len();
        let step = match ty {
            RustType::Bool => PrintStep::PrintBool { argument },
            RustType::I64 => PrintStep::PrintI64 { argument },
            RustType::GoString => PrintStep::PrintGoString { argument },
            RustType::Unit => return None,
        };
        steps.push(step);
        if newline && !last {
            steps.push(PrintStep::PrintSpace);
        }
    }
    if newline {
        steps.push(PrintStep::PrintNewline);
    }
    Some(steps)
}
