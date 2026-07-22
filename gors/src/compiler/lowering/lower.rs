//! Mandatory conversion from normalized Go MIR to explicit Rust IR.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::mir;
use crate::compiler::rust_ir as out;
use crate::compiler::types::{ConstValue, IntTy, Signature as GoSignature, Ty};

#[cfg(test)]
pub(super) fn lower_file(file: mir::File) -> Result<out::File, Diagnostic> {
    let executable_package = file.package == "main";
    Ok(out::File {
        package: file.package,
        functions: file
            .functions
            .into_iter()
            .map(|function| lower_function(function, executable_package))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(super) fn lower_function(
    function: mir::Function,
    executable_package: bool,
) -> Result<out::Function, Diagnostic> {
    let artifact = if executable_package && function.name == "main" {
        out::FunctionArtifactPlan::executable_entrypoint()
    } else {
        out::FunctionArtifactPlan::public_definition(function.id)
    };
    let signature = lower_signature(&function.signature)?;
    let locals = function
        .locals
        .into_iter()
        .map(|local| {
            let initialization = function
                .params
                .iter()
                .position(|parameter| *parameter == local.id)
                .map_or(
                    out::SlotInitialization::Uninitialized,
                    out::SlotInitialization::Parameter,
                );
            Ok(out::LocalDecl {
                id: local.id,
                name: local.name,
                ty: lower_type(&local.ty)?,
                storage: out::StorageClass::CheckedOptionSlot,
                initialization,
            })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let blocks = function
        .blocks
        .into_iter()
        .map(|block| lower_block(block, &locals))
        .collect::<Result<Vec<_>, _>>()?;
    let mut lowered = out::Function {
        id: function.id,
        name: function.name,
        artifact,
        signature,
        parameters: function.params,
        locals,
        blocks,
        entry: function.entry,
        control_flow: out::ControlFlowPlan::PcDispatchU32,
        source: function.source,
    };
    out::select_read_operations(&mut lowered)?;
    Ok(lowered)
}

pub(super) fn lower_signature(signature: &GoSignature) -> Result<out::Signature, Diagnostic> {
    Ok(out::Signature {
        params: signature
            .params
            .iter()
            .map(lower_type)
            .collect::<Result<Vec<_>, _>>()?,
        results: signature
            .results
            .iter()
            .map(lower_type)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn lower_block(
    block: mir::BasicBlock,
    locals: &[out::LocalDecl],
) -> Result<out::BasicBlock, Diagnostic> {
    Ok(out::BasicBlock {
        id: block.id,
        provenance: lower_provenance(block.provenance),
        statements: block
            .statements
            .into_iter()
            .map(|statement| lower_statement(statement, locals))
            .collect::<Result<Vec<_>, _>>()?,
        terminator: lower_terminator(block.terminator, locals)?,
    })
}

fn lower_statement(
    statement: mir::Statement,
    locals: &[out::LocalDecl],
) -> Result<out::Statement, Diagnostic> {
    let value = lower_rvalue(statement.value, locals)?;
    Ok(out::Statement {
        destination: lower_place(statement.destination),
        effects: out::statement_effects(&value),
        value,
        store: out::StoreOp::SetSome,
        provenance: lower_provenance(statement.provenance),
    })
}

fn lower_rvalue(rvalue: mir::Rvalue, locals: &[out::LocalDecl]) -> Result<out::Rvalue, Diagnostic> {
    let kind = match rvalue.kind {
        mir::RvalueKind::Use(operand) => out::RvalueKind::Use(lower_operand(operand, locals)?),
        mir::RvalueKind::Unary { op, operand, ty } => {
            let operand_ty = mir_operand_type(&operand, locals)?;
            out::RvalueKind::Unary {
                op: lower_unary_op(op, operand_ty, lower_type(&ty)?)?,
                operand: lower_operand(operand, locals)?,
            }
        }
        mir::RvalueKind::Binary {
            op,
            left,
            right,
            ty,
        } => {
            let left_ty = mir_operand_type(&left, locals)?;
            let right_ty = mir_operand_type(&right, locals)?;
            out::RvalueKind::Binary {
                op: lower_binary_op(op, left_ty, right_ty, lower_type(&ty)?)?,
                left: lower_operand(left, locals)?,
                right: lower_operand(right, locals)?,
            }
        }
    };
    let effects = out::rvalue_effects(&kind);
    Ok(out::Rvalue {
        kind,
        effects,
        panic: out::panic_edge(effects),
        provenance: lower_provenance(rvalue.provenance),
    })
}

fn lower_terminator(
    terminator: mir::Terminator,
    locals: &[out::LocalDecl],
) -> Result<out::Terminator, Diagnostic> {
    let kind = match terminator.kind {
        mir::TerminatorKind::Goto(target) => out::TerminatorKind::Goto(target),
        mir::TerminatorKind::SwitchBool {
            condition,
            then_target,
            else_target,
        } => out::TerminatorKind::SwitchBool {
            condition: lower_operand(condition, locals)?,
            then_target,
            else_target,
        },
        mir::TerminatorKind::Call {
            callee,
            args,
            destination,
            target: next,
        } => {
            let argument_types = args
                .iter()
                .map(|argument| mir_operand_type(argument, locals))
                .collect::<Result<Vec<_>, _>>()?;
            let call_target = match callee {
                hir::Callee::Function(id) => out::CallTarget::Function(id),
                hir::Callee::Builtin(builtin) => out::CallTarget::RuntimePrint {
                    steps: out::print_plan(&argument_types, builtin == hir::Builtin::Println)
                        .ok_or_else(|| {
                            Diagnostic::backend(
                                "unit value reached Rust print representation lowering",
                            )
                        })?,
                },
            };
            out::TerminatorKind::Call {
                target: call_target,
                args: args
                    .into_iter()
                    .map(|argument| lower_operand(argument, locals))
                    .collect::<Result<Vec<_>, _>>()?,
                destination: destination.map(lower_place),
                next,
            }
        }
        mir::TerminatorKind::Return(values) => out::TerminatorKind::Return(
            values
                .into_iter()
                .map(|value| lower_operand(value, locals))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        mir::TerminatorKind::Unreachable => out::TerminatorKind::Unreachable,
    };
    let effects = out::terminator_effects(&kind);
    Ok(out::Terminator {
        kind,
        effects,
        panic: out::panic_edge(effects),
        provenance: lower_provenance(terminator.provenance),
    })
}

fn lower_operand(
    operand: mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::Operand, Diagnostic> {
    match operand {
        mir::Operand::Read(place) => {
            let place = lower_place(place);
            let local = local(locals, place.local)?;
            let op = local.ty.conservative_read_op().ok_or_else(|| {
                Diagnostic::backend(format!(
                    "Rust lowering cannot read unit local {}",
                    place.local.0
                ))
            })?;
            Ok(out::Operand::Read { place, op })
        }
        mir::Operand::Constant(value, ty) => {
            Ok(out::Operand::Constant(lower_constant(value, &ty)?))
        }
        mir::Operand::Unit => Ok(out::Operand::Unit),
    }
}

fn lower_constant(value: ConstValue, ty: &Ty) -> Result<out::Constant, Diagnostic> {
    match (value, ty) {
        (ConstValue::Bool(value), Ty::Bool) => Ok(out::Constant::Bool(value)),
        (ConstValue::Int(value), Ty::Int(IntTy::Int)) => value
            .parse::<i64>()
            .map(out::Constant::I64)
            .map_err(|_| Diagnostic::backend(format!("Go int is outside Rust IR i64: {value}"))),
        (ConstValue::String(value), Ty::String) => Ok(out::Constant::GoString(value)),
        (value, ty) => Err(Diagnostic::backend(format!(
            "invalid constant reached Rust lowering: {value:?} as {ty:?}"
        ))),
    }
}

fn lower_unary_op(
    op: hir::UnaryOp,
    operand: out::RustType,
    result: out::RustType,
) -> Result<out::UnaryOp, Diagnostic> {
    let lowered = match (op, operand, result) {
        (hir::UnaryOp::Positive, out::RustType::I64, out::RustType::I64) => out::UnaryOp::Identity,
        (hir::UnaryOp::Negative, out::RustType::I64, out::RustType::I64) => out::UnaryOp::IntNeg,
        (hir::UnaryOp::Not, out::RustType::Bool, out::RustType::Bool) => out::UnaryOp::BoolNot,
        (hir::UnaryOp::BitNot, out::RustType::I64, out::RustType::I64) => out::UnaryOp::IntBitNot,
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid unary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

fn lower_binary_op(
    op: hir::BinaryOp,
    left: out::RustType,
    right: out::RustType,
    result: out::RustType,
) -> Result<out::BinaryOp, Diagnostic> {
    use hir::BinaryOp as Go;
    use out::BinaryOp as Rust;
    use out::RustType::{Bool, GoString, I64};
    let lowered = match (op, left, right, result) {
        (Go::Add, I64, I64, I64) => Rust::IntAdd,
        (Go::Sub, I64, I64, I64) => Rust::IntSub,
        (Go::Mul, I64, I64, I64) => Rust::IntMul,
        (Go::Div, I64, I64, I64) => Rust::IntDiv,
        (Go::Rem, I64, I64, I64) => Rust::IntRem,
        (Go::BitAnd, I64, I64, I64) => Rust::IntBitAnd,
        (Go::BitOr, I64, I64, I64) => Rust::IntBitOr,
        (Go::BitXor, I64, I64, I64) => Rust::IntBitXor,
        (Go::Shl, I64, I64, I64) => Rust::IntShl,
        (Go::Shr, I64, I64, I64) => Rust::IntShr,
        (Go::AndNot, I64, I64, I64) => Rust::IntAndNot,
        (Go::Equal, Bool, Bool, Bool) => Rust::BoolEqual,
        (Go::NotEqual, Bool, Bool, Bool) => Rust::BoolNotEqual,
        (Go::Equal, I64, I64, Bool) => Rust::IntEqual,
        (Go::NotEqual, I64, I64, Bool) => Rust::IntNotEqual,
        (Go::Less, I64, I64, Bool) => Rust::IntLess,
        (Go::LessEqual, I64, I64, Bool) => Rust::IntLessEqual,
        (Go::Greater, I64, I64, Bool) => Rust::IntGreater,
        (Go::GreaterEqual, I64, I64, Bool) => Rust::IntGreaterEqual,
        (Go::Add, GoString, GoString, GoString) => Rust::StringConcat,
        (Go::Equal, GoString, GoString, Bool) => Rust::StringEqual,
        (Go::NotEqual, GoString, GoString, Bool) => Rust::StringNotEqual,
        (Go::Less, GoString, GoString, Bool) => Rust::StringLess,
        (Go::LessEqual, GoString, GoString, Bool) => Rust::StringLessEqual,
        (Go::Greater, GoString, GoString, Bool) => Rust::StringGreater,
        (Go::GreaterEqual, GoString, GoString, Bool) => Rust::StringGreaterEqual,
        invalid => {
            return Err(Diagnostic::backend(format!(
                "invalid binary representation lowering: {invalid:?}"
            )));
        }
    };
    Ok(lowered)
}

fn lower_type(ty: &Ty) -> Result<out::RustType, Diagnostic> {
    match ty.default_typed() {
        Ty::Unit => Ok(out::RustType::Unit),
        Ty::Bool => Ok(out::RustType::Bool),
        Ty::Int(IntTy::Int) => Ok(out::RustType::I64),
        Ty::String => Ok(out::RustType::GoString),
        unsupported => Err(Diagnostic::backend(format!(
            "unsupported Go type reached Rust lowering: {unsupported:?}"
        ))),
    }
}

fn mir_operand_type(
    operand: &mir::Operand,
    locals: &[out::LocalDecl],
) -> Result<out::RustType, Diagnostic> {
    match operand {
        mir::Operand::Read(place) => Ok(local(locals, place.local)?.ty),
        mir::Operand::Constant(_, ty) => lower_type(ty),
        mir::Operand::Unit => Ok(out::RustType::Unit),
    }
}

fn local(locals: &[out::LocalDecl], id: out::LocalId) -> Result<&out::LocalDecl, Diagnostic> {
    locals
        .get(id.0 as usize)
        .ok_or_else(|| Diagnostic::backend(format!("invalid local during Rust lowering: {}", id.0)))
}

fn lower_place(place: mir::Place) -> out::Place {
    out::Place { local: place.local }
}

fn lower_provenance(provenance: mir::Provenance) -> out::Provenance {
    match provenance {
        mir::Provenance::Source(source) => out::Provenance::Source(source),
        mir::Provenance::Synthetic(mir::SyntheticOrigin::NamedResultInitialization) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::NamedResultInitialization)
        }
        mir::Provenance::Synthetic(mir::SyntheticOrigin::ImplicitReturn) => {
            out::Provenance::Synthetic(out::SyntheticOrigin::ImplicitReturn)
        }
    }
}
