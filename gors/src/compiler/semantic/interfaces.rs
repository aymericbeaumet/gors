//! Structural interface satisfaction and explicit dynamic-value construction.

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, SyntaxSource};
use crate::compiler::types::{ConstValue, InterfaceMethod, Signature, Ty};

use super::FunctionLowerer;
use super::expressions::{
    coerce_expr, default_expr_type, ensure_bootstrap_value_type, is_assignable,
};
use super::lower_type;

pub(super) fn error_interface_ty() -> Ty {
    Ty::Interface(vec![InterfaceMethod {
        name: "Error".to_owned(),
        signature: Signature {
            params: Vec::new(),
            results: vec![Ty::String],
            variadic: false,
        },
    }])
}

impl FunctionLowerer {
    pub(super) fn try_lower_interface_comma_ok(
        &mut self,
        expression: &ExprSyntax,
    ) -> Option<Result<hir::Expr, Diagnostic>> {
        let ExprSyntaxKind::TypeAssert { value, asserted } = &expression.kind else {
            return None;
        };
        Some((|| {
            let node = self.alloc_node(expression.source)?;
            let source = SourceRef::node(node);
            self.lower_interface_type_assertion(value, asserted.as_deref(), true, node, source)
        })())
    }

    pub(super) fn lower_interface_type_assertion(
        &mut self,
        value: &ExprSyntax,
        asserted: Option<&ExprSyntax>,
        comma_ok: bool,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let asserted = asserted.ok_or_else(|| {
            Diagnostic::semantic("x.(type) is only valid in a type switch", source)
        })?;
        let interface = self.lower_expr(value, None)?;
        self.build_interface_type_assertion(interface, asserted, comma_ok, node, source)
    }

    pub(super) fn build_interface_type_assertion(
        &mut self,
        interface: hir::Expr,
        asserted: &ExprSyntax,
        comma_ok: bool,
        node: NodeId,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        let (asserted_ty, type_identity) =
            self.interface_case_identity(&interface.ty, asserted, source)?;
        let identity = self.interface_identity_expr(asserted.source, type_identity)?;
        let effects = interface.effects.union(hir::Effects {
            may_call: true,
            may_panic: !comma_ok,
            ..hir::Effects::default()
        });
        let ty = if comma_ok {
            Ty::Tuple(vec![asserted_ty, Ty::Bool])
        } else {
            asserted_ty
        };
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::InterfaceAssert),
                args: vec![interface, identity],
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        })
    }

    pub(super) fn build_interface_type_test(
        &mut self,
        interface: hir::Expr,
        asserted: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
    ) -> Result<(Ty, hir::Expr), Diagnostic> {
        let (asserted_ty, type_identity) =
            self.interface_case_identity(&interface.ty, asserted, source)?;
        let identity = self.interface_identity_expr(asserted.source, type_identity)?;
        let effects = interface.effects.union(hir::Effects {
            may_call: true,
            ..hir::Effects::default()
        });
        let test = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::InterfaceIsType),
                args: vec![interface, identity],
            },
            ty: Ty::Bool,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        Ok((asserted_ty, test))
    }

    fn interface_case_identity(
        &self,
        interface_ty: &Ty,
        asserted: &ExprSyntax,
        source: SourceRef,
    ) -> Result<(Ty, Vec<u8>), Diagnostic> {
        let Ty::Interface(required_methods) = interface_ty.underlying() else {
            return Err(Diagnostic::semantic(
                "type assertion requires an interface value",
                source,
            ));
        };
        let asserted_ty = lower_type(asserted, &self.type_aliases, source)?;
        if matches!(asserted_ty.underlying(), Ty::Interface(_)) {
            return Err(Diagnostic::unsupported(
                "assertions to interface types require interface satisfaction dispatch",
                source,
            ));
        }
        ensure_bootstrap_value_type(&asserted_ty, source)?;
        if !supports_dynamic_interface_type(&asserted_ty) {
            return Err(Diagnostic::unsupported(
                format!("type assertions do not yet support {asserted_ty:?}"),
                source,
            ));
        }
        if !self.concrete_implements(&asserted_ty, required_methods) {
            return Err(Diagnostic::semantic(
                format!(
                    "impossible type assertion: {asserted_ty:?} does not implement {:?}",
                    interface_ty
                ),
                source,
            ));
        }
        let type_identity = asserted_ty.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::unsupported(
                format!("type assertions do not yet support {asserted_ty:?}"),
                source,
            )
        })?;
        Ok((asserted_ty, type_identity))
    }

    fn interface_identity_expr(
        &mut self,
        syntax_source: SyntaxSource,
        type_identity: Vec<u8>,
    ) -> Result<hir::Expr, Diagnostic> {
        let identity_node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node: identity_node,
            kind: hir::ExprKind::Constant(ConstValue::String(type_identity)),
            ty: Ty::String,
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(identity_node),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_interface_selector_call(
        &mut self,
        receiver: hir::Expr,
        member: &str,
        member_source: SyntaxSource,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let signature = interface_method_signature(&receiver.ty, member, source)?;
        let args = self.lower_call_arguments(
            arguments,
            &signature.params,
            signature.variadic,
            spread,
            member_source,
            source,
            "method",
        )?;
        self.build_interface_call(
            receiver,
            member,
            args,
            node,
            source,
            expected,
            allow_discarded_call_result,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_interface_call(
        &mut self,
        receiver: hir::Expr,
        member: &str,
        args: Vec<hir::Expr>,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let signature = interface_method_signature(&receiver.ty, member, source)?;
        if args.len() != signature.params.len() {
            return Err(Diagnostic::backend(
                "interface call argument arity changed during semantic lowering",
            ));
        }
        if args
            .iter()
            .zip(&signature.params)
            .any(|(argument, expected)| &argument.ty != expected)
        {
            return Err(Diagnostic::backend(
                "interface call argument type changed during semantic lowering",
            ));
        }
        let candidates = self.interface_call_candidates(&receiver.ty, member, source)?;
        let ty = match signature.results.as_slice() {
            [] => Ty::Unit,
            [single] => single.clone(),
            _ => {
                return Err(Diagnostic::unsupported(
                    "multi-result interface method values require tuple dispatch lowering",
                    source,
                ));
            }
        };
        if ty == Ty::Unit && !allow_discarded_call_result {
            return Err(Diagnostic::unsupported(
                "a no-result method call cannot be used as a value",
                source,
            ));
        }
        let effects = args.iter().fold(
            receiver.effects.union(hir::Effects {
                may_call: true,
                may_allocate: true,
                may_block: true,
                may_panic: true,
                may_write: true,
                ..hir::Effects::default()
            }),
            |effects, argument| effects.union(argument.effects),
        );
        let mut lowered = hir::Expr {
            node,
            kind: hir::ExprKind::InterfaceCall {
                receiver: Box::new(receiver),
                args,
                candidates,
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            super::expressions::coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }

    pub(super) fn coerce_interface_value(
        &mut self,
        mut value: hir::Expr,
        expected: &Ty,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        let source = value.source;
        let Ty::Interface(expected_methods) = expected.underlying() else {
            return Err(Diagnostic::backend(
                "interface coercion received a non-interface destination",
            ));
        };
        if let Ty::Interface(actual_methods) = value.ty.underlying() {
            if !interface_contains(actual_methods, expected_methods) {
                return Err(Diagnostic::semantic(
                    format!("cannot use {:?} as {expected:?}", value.ty),
                    source,
                ));
            }
            value.ty = expected.clone();
            return Ok(value);
        }
        if matches!(value.ty, Ty::Untyped(_)) {
            value = default_expr_type(value, source)?;
        }
        ensure_bootstrap_value_type(&value.ty, source)?;
        if !self.concrete_implements(&value.ty, expected_methods) {
            return Err(Diagnostic::semantic(
                format!("type {:?} does not implement {expected:?}", value.ty),
                source,
            ));
        }
        let type_identity = value.ty.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::unsupported(
                format!(
                    "interface values do not yet support dynamic type {:?}",
                    value.ty
                ),
                source,
            )
        })?;
        let effects = value.effects.union(hir::Effects {
            may_call: true,
            may_allocate: value.ty.bootstrap_i64_struct_fields().is_some(),
            ..hir::Effects::default()
        });
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::InterfaceValue {
                value: Box::new(value),
                type_identity,
            },
            ty: expected.clone(),
            category: hir::ValueCategory::Value,
            effects,
            source: SourceRef::node(node),
        })
    }

    pub(super) fn assignment_value_coercion(
        &self,
        actual: &Ty,
        expected: &Ty,
        source: SourceRef,
    ) -> Result<hir::ValueCoercion, Diagnostic> {
        if actual == expected {
            return Ok(hir::ValueCoercion::Identity);
        }
        let Ty::Interface(expected_methods) = expected.underlying() else {
            if is_assignable(actual, expected) {
                return Ok(hir::ValueCoercion::Representation {
                    target: expected.clone(),
                });
            }
            return Err(Diagnostic::semantic(
                format!("type {actual:?} is not assignable to {expected:?}"),
                source,
            ));
        };
        if let Ty::Interface(actual_methods) = actual.underlying() {
            if !interface_contains(actual_methods, expected_methods) {
                return Err(Diagnostic::semantic(
                    format!("type {actual:?} is not assignable to {expected:?}"),
                    source,
                ));
            }
            return Ok(hir::ValueCoercion::Representation {
                target: expected.clone(),
            });
        }
        ensure_bootstrap_value_type(actual, source)?;
        if !self.concrete_implements(actual, expected_methods) {
            return Err(Diagnostic::semantic(
                format!("type {actual:?} does not implement {expected:?}"),
                source,
            ));
        }
        let type_identity = actual.dynamic_type_identity().ok_or_else(|| {
            Diagnostic::unsupported(
                format!("interface values do not yet support dynamic type {actual:?}"),
                source,
            )
        })?;
        Ok(hir::ValueCoercion::Interface {
            target: expected.clone(),
            type_identity,
        })
    }

    pub(super) fn apply_assignment_value_coercion(
        &mut self,
        mut value: hir::Expr,
        coercion: &hir::ValueCoercion,
        syntax_source: SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        match coercion {
            hir::ValueCoercion::Identity => Ok(value),
            hir::ValueCoercion::Representation { target } => {
                let source = value.source;
                coerce_expr(&mut value, target, source)?;
                Ok(value)
            }
            hir::ValueCoercion::Interface { target, .. } => {
                self.coerce_interface_value(value, target, syntax_source)
            }
        }
    }

    pub(super) fn concrete_implements(&self, concrete: &Ty, required: &[InterfaceMethod]) -> bool {
        if required.is_empty() {
            return true;
        }
        let Some((definition, pointer)) = receiver_definition(concrete) else {
            return false;
        };
        required.iter().all(|required| {
            self.methods
                .get(&(definition, required.name.clone()))
                .is_some_and(|method| {
                    (pointer || !method.pointer_receiver)
                        && method.signature.variadic == required.signature.variadic
                        && method.signature.params.get(1..) == Some(&required.signature.params)
                        && method.signature.results == required.signature.results
                })
        })
    }

    fn interface_call_candidates(
        &self,
        interface_ty: &Ty,
        member: &str,
        source: SourceRef,
    ) -> Result<Vec<hir::InterfaceCallCandidate>, Diagnostic> {
        let Ty::Interface(required) = interface_ty.underlying() else {
            return Err(Diagnostic::backend(
                "interface dispatch candidate search received a non-interface type",
            ));
        };
        let mut candidates = std::collections::BTreeMap::new();
        for ty in self.type_aliases.values() {
            let Ty::Named { definition, .. } = ty else {
                continue;
            };
            let Some(method) = self.methods.get(&(*definition, member.to_owned())) else {
                continue;
            };
            let receiver_ty = method
                .signature
                .params
                .first()
                .cloned()
                .ok_or_else(|| Diagnostic::backend("method signature omitted its receiver"))?;
            for dynamic_ty in [ty.clone(), Ty::Pointer(Box::new(ty.clone()))] {
                if !supports_dynamic_interface_type(&dynamic_ty)
                    || !self.concrete_implements(&dynamic_ty, required)
                {
                    continue;
                }
                let Some(type_identity) = dynamic_ty.dynamic_type_identity() else {
                    continue;
                };
                candidates.insert(
                    type_identity.clone(),
                    hir::InterfaceCallCandidate {
                        type_identity,
                        dynamic_ty,
                        receiver_ty: receiver_ty.clone(),
                        function: method.id,
                    },
                );
            }
        }
        if candidates.is_empty() {
            return Err(Diagnostic::unsupported(
                format!("interface method {member} has no executable concrete implementations"),
                source,
            ));
        }
        Ok(candidates.into_values().collect())
    }
}

pub(super) fn interface_method_signature(
    interface_ty: &Ty,
    member: &str,
    source: SourceRef,
) -> Result<Signature, Diagnostic> {
    let Ty::Interface(methods) = interface_ty.underlying() else {
        return Err(Diagnostic::backend(
            "interface method call has a non-interface receiver",
        ));
    };
    methods
        .iter()
        .find(|method| method.name == member)
        .map(|method| method.signature.clone())
        .ok_or_else(|| {
            Diagnostic::semantic(
                format!("type {interface_ty:?} has no method {member}"),
                source,
            )
        })
}

fn interface_contains(actual: &[InterfaceMethod], required: &[InterfaceMethod]) -> bool {
    required.iter().all(|required| {
        actual
            .iter()
            .any(|actual| actual.name == required.name && actual.signature == required.signature)
    })
}

fn receiver_definition(ty: &Ty) -> Option<(crate::compiler::ids::DefId, bool)> {
    match ty {
        Ty::Named { definition, .. } => Some((*definition, false)),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => Some((*definition, true)),
            _ => None,
        },
        _ => None,
    }
}

fn supports_dynamic_interface_type(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Bool | Ty::Int(crate::compiler::types::IntTy::Int) | Ty::String
    ) || ty.bootstrap_i64_struct_fields().is_some()
        || ty.bootstrap_i64_struct_pointer_fields().is_some()
}
