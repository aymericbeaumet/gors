//! MIR inlining for directly called, non-escaping function literals.

use super::super::construct::{
    call_effects, make_rvalue, make_statement, make_terminator, operand_ty,
};
use super::super::{Operand, Place, Provenance, RvalueKind, TerminatorKind};
use super::{ClosureReturn, FunctionLowerer};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::ClosureId;
use crate::compiler::provenance::SourceRef;

impl FunctionLowerer {
    pub(super) fn lower_call_into(
        &mut self,
        expression: &hir::Expr,
        destinations: Vec<Place>,
    ) -> Result<(), Diagnostic> {
        let hir::ExprKind::Call { callee, args } = &expression.kind else {
            return Err(Diagnostic::backend(
                "tuple-valued non-call reached MIR call lowering",
            ));
        };
        if let hir::Callee::Closure(id) = callee {
            return self.lower_closure_call(*id, args, destinations, expression.source);
        }
        let mut operands = Vec::with_capacity(args.len());
        for argument in args {
            let operand = self.lower_expr(argument)?;
            operands.push(self.materialize(
                operand,
                argument.ty.clone(),
                Provenance::Source(argument.source),
            )?);
        }
        let provenance = Provenance::Source(expression.source);
        let target = self.new_block(provenance.clone());
        self.terminate(make_terminator(
            TerminatorKind::Call {
                callee: *callee,
                args: operands,
                destinations,
                target,
            },
            call_effects(),
            provenance,
        ))?;
        self.current = target;
        Ok(())
    }

    pub(super) fn lower_closure_call(
        &mut self,
        id: ClosureId,
        arguments: &[hir::Expr],
        destinations: Vec<Place>,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let closure = self
            .closures
            .get(id.index() as usize)
            .filter(|closure| closure.id == id)
            .cloned()
            .ok_or_else(|| Diagnostic::backend(format!("unknown local function {}", id.0)))?;
        if self.active_closures.contains(&id) {
            return Err(Diagnostic::unsupported(
                "recursive local function literals are not yet implemented",
                source,
            ));
        }
        if arguments.len() != closure.params.len() {
            return Err(Diagnostic::backend(
                "local function argument arity changed before MIR lowering",
            ));
        }
        if destinations.len() != closure.signature.results.len() {
            return Err(Diagnostic::backend(
                "local function result arity changed before MIR lowering",
            ));
        }

        let mut operands = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let operand = self.lower_expr(argument)?;
            operands.push(self.materialize(
                operand,
                argument.ty.clone(),
                Provenance::Source(argument.source),
            )?);
        }
        for (parameter, operand) in closure.params.iter().zip(operands) {
            let parameter_ty = self.local_ty(*parameter)?.clone();
            if parameter_ty != operand_ty(&operand, &self.locals)? {
                return Err(Diagnostic::backend(
                    "local function argument type changed before MIR lowering",
                ));
            }
            self.assign_closure_place(Place { local: *parameter }, operand, source)?;
        }
        for (result, ty) in closure.named_results.iter().zip(&closure.signature.results) {
            if let Some(result) = result {
                let zero = ty.zero().ok_or_else(|| {
                    Diagnostic::backend(format!(
                        "local function named result {ty:?} has no zero value"
                    ))
                })?;
                self.assign_closure_place(
                    Place { local: *result },
                    Operand::Constant(zero, ty.clone()),
                    source,
                )?;
            }
        }

        let continuation = self.new_block(Provenance::Source(source));
        self.closure_returns.push(ClosureReturn {
            destinations,
            target: continuation,
            named_results: closure.named_results.clone(),
        });
        self.active_closures.push(id);
        let lowered = self.lower_block(&closure.body);
        self.active_closures.pop();
        let context = self
            .closure_returns
            .pop()
            .ok_or_else(|| Diagnostic::backend("local function return context disappeared"))?;
        lowered?;

        if !self.is_terminated(self.current)? {
            if closure.signature.results.is_empty() {
                self.terminate(make_terminator(
                    TerminatorKind::Goto(context.target),
                    hir::Effects::default(),
                    Provenance::Source(source),
                ))?;
            } else {
                return Err(Diagnostic::backend(
                    "result-bearing local function can reach its end without returning",
                ));
            }
        }
        self.current = context.target;
        Ok(())
    }

    pub(super) fn lower_closure_return(
        &mut self,
        values: &[hir::Expr],
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let context = self
            .closure_returns
            .last()
            .cloned()
            .ok_or_else(|| Diagnostic::backend("local function return has no call context"))?;
        let operands = self.lower_return_values(values)?;
        if operands.len() != context.destinations.len() {
            return Err(Diagnostic::backend(
                "local function return arity changed before MIR lowering",
            ));
        }
        if !context.named_results.is_empty() && context.named_results.len() != operands.len() {
            return Err(Diagnostic::backend(
                "local function named-result arity changed before MIR lowering",
            ));
        }

        let returned = if context.named_results.is_empty() {
            operands
        } else {
            let mut returned = Vec::with_capacity(operands.len());
            for (named_result, operand) in context.named_results.iter().zip(operands) {
                if let Some(local) = named_result {
                    self.assign_closure_place(Place { local: *local }, operand, source)?;
                    returned.push(Operand::Read(Place { local: *local }));
                } else {
                    returned.push(operand);
                }
            }
            returned
        };
        for (destination, operand) in context.destinations.iter().copied().zip(returned) {
            self.assign_closure_place(destination, operand, source)?;
        }
        self.terminate(make_terminator(
            TerminatorKind::Goto(context.target),
            hir::Effects::default(),
            Provenance::Source(source),
        ))
    }

    fn assign_closure_place(
        &mut self,
        destination: Place,
        operand: Operand,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        let provenance = Provenance::Source(source);
        let value = make_rvalue(
            RvalueKind::Use(operand),
            hir::Effects::default(),
            provenance.clone(),
        );
        self.push_statement(make_statement(destination, value, provenance))
    }
}
