use super::super::FunctionLowerer;
use super::super::expressions::is_assignable;
use super::{CheckedOperand, LengthCapacityClass, LengthCapacityOp, classify, is_nilable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, Signature, Ty, UntypedTy};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn source_contains_call_or_receive(&self, expression: &ExprSyntax) -> bool {
        match &expression.kind {
            ExprSyntaxKind::Paren(inner) => self.source_contains_call_or_receive(inner),
            ExprSyntaxKind::Unary {
                token: Token::ARROW,
                ..
            } => true,
            ExprSyntaxKind::Unary { expression, .. } => {
                self.source_contains_call_or_receive(expression)
            }
            ExprSyntaxKind::Binary { left, right, .. } => {
                self.source_contains_call_or_receive(left)
                    || self.source_contains_call_or_receive(right)
            }
            ExprSyntaxKind::Call {
                callee,
                arguments,
                spread,
            } => {
                if self.is_type_expression(callee) {
                    arguments
                        .iter()
                        .any(|argument| self.source_contains_call_or_receive(argument))
                } else if let ExprSyntaxKind::Ident(identifier) = &callee.kind
                    && self.is_predeclared_constant_builtin(identifier.name.as_ref())
                {
                    let name = identifier.name.as_ref();
                    if matches!(name, "len" | "cap")
                        && !spread
                        && let [operand] = arguments.as_ref()
                        && let Ok(checked) = self.check_length_capacity_operand(
                            operand,
                            SourceRef::definition(self.owner),
                            None,
                        )
                    {
                        let operation = if name == "len" {
                            LengthCapacityOp::Len
                        } else {
                            LengthCapacityOp::Cap
                        };
                        return !matches!(
                            classify(
                                operation,
                                &checked.ty,
                                checked.constant.as_ref(),
                                checked.contains_call_or_receive,
                                SourceRef::definition(self.owner),
                            ),
                            Ok(LengthCapacityClass::Constant(_))
                        );
                    }
                    self.eval_constant_expression(
                        expression,
                        SourceRef::definition(self.owner),
                        None,
                    )
                    .is_err()
                } else {
                    true
                }
            }
            ExprSyntaxKind::Selector { base, .. } => self.source_contains_call_or_receive(base),
            ExprSyntaxKind::TypeAssert { value, .. } => self.source_contains_call_or_receive(value),
            ExprSyntaxKind::KeyValue { key, value } => {
                self.source_contains_call_or_receive(key)
                    || self.source_contains_call_or_receive(value)
            }
            ExprSyntaxKind::CompositeLiteral { elements, .. } => elements
                .iter()
                .any(|element| self.source_contains_call_or_receive(element)),
            ExprSyntaxKind::Index { base, index } => {
                self.source_contains_call_or_receive(base)
                    || self.source_contains_call_or_receive(index)
            }
            ExprSyntaxKind::IndexList { base, indices } => {
                self.source_contains_call_or_receive(base)
                    || indices
                        .iter()
                        .any(|index| self.source_contains_call_or_receive(index))
            }
            ExprSyntaxKind::Slice {
                base,
                low,
                high,
                max,
            } => {
                self.source_contains_call_or_receive(base)
                    || low
                        .iter()
                        .chain(high.iter())
                        .chain(max.iter())
                        .any(|bound| self.source_contains_call_or_receive(bound))
            }
            ExprSyntaxKind::Ident(_)
            | ExprSyntaxKind::Literal { .. }
            | ExprSyntaxKind::FunctionLiteral { .. }
            | ExprSyntaxKind::FunctionType { .. }
            | ExprSyntaxKind::ArrayType { .. }
            | ExprSyntaxKind::MapType { .. }
            | ExprSyntaxKind::ChannelType { .. }
            | ExprSyntaxKind::StructType { .. }
            | ExprSyntaxKind::InterfaceType { .. }
            | ExprSyntaxKind::Unsupported(_) => false,
        }
    }

    pub(super) fn check_length_capacity_identifier(
        &self,
        name: &str,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<CheckedOperand, Diagnostic> {
        if let Some(local) = self.lookup_local(name) {
            return Ok(CheckedOperand {
                ty: self.place_ty(hir::Place::Local(local))?.clone(),
                constant: None,
                contains_call_or_receive: false,
            });
        }
        if let Some(constant) = self.lookup_local_constant(name) {
            return Ok(CheckedOperand {
                ty: constant.ty.clone(),
                constant: Some(constant.value.clone()),
                contains_call_or_receive: false,
            });
        }
        if let Some(constant) = self.constants.get(name) {
            return Ok(CheckedOperand {
                ty: constant.ty.clone(),
                constant: Some(constant.value.clone()),
                contains_call_or_receive: false,
            });
        }
        if let Some(variable) = self.variables.get(name) {
            return Ok(CheckedOperand {
                ty: variable.ty.clone(),
                constant: None,
                contains_call_or_receive: false,
            });
        }
        if matches!(name, "true" | "false") && self.resolves_to_predeclared(name) {
            return Ok(CheckedOperand {
                ty: Ty::Untyped(crate::compiler::types::UntypedTy::Bool),
                constant: Some(ConstValue::Bool(name == "true")),
                contains_call_or_receive: false,
            });
        }
        if name == "iota" && self.resolves_to_predeclared(name) {
            let Some(iota) = iota else {
                return Err(Diagnostic::semantic(
                    "iota is only defined in constant declarations",
                    source,
                ));
            };
            return Ok(CheckedOperand {
                ty: Ty::Untyped(UntypedTy::Int),
                constant: Some(ConstValue::Int(iota.to_string())),
                contains_call_or_receive: false,
            });
        }
        Err(Diagnostic::semantic(
            format!("undefined identifier {name}"),
            source,
        ))
    }

    pub(super) fn check_length_capacity_call(
        &self,
        callee: &ExprSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        whole: &ExprSyntax,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<CheckedOperand, Diagnostic> {
        if self.is_type_expression(callee) {
            if spread || arguments.len() != 1 {
                return Err(Diagnostic::semantic(
                    "a conversion requires exactly one argument",
                    source,
                ));
            }
            let target = self.lower_length_capacity_type(callee, source, iota)?;
            let argument = arguments
                .first()
                .ok_or_else(|| Diagnostic::backend("conversion argument disappeared"))?;
            if self.is_predeclared_nil_identifier(argument) {
                if !is_nilable(&target) {
                    return Err(Diagnostic::semantic(
                        format!("cannot convert nil to {target:?}"),
                        source,
                    ));
                }
                return Ok(CheckedOperand {
                    ty: target,
                    constant: None,
                    contains_call_or_receive: false,
                });
            }
            let argument = self.check_length_capacity_operand(argument, source, iota)?;
            if !is_assignable(&argument.ty, &target)
                && argument.ty.underlying() != target.underlying()
                && !(argument.ty.is_numeric() && target.is_numeric())
            {
                return Err(Diagnostic::semantic(
                    format!("cannot convert {:?} to {target:?}", argument.ty),
                    source,
                ));
            }
            return Ok(CheckedOperand {
                ty: target,
                constant: argument.constant,
                contains_call_or_receive: argument.contains_call_or_receive,
            });
        }

        if let ExprSyntaxKind::Ident(identifier) = &callee.kind {
            let name = identifier.name.as_ref();
            if self.is_predeclared_constant_builtin(name) {
                for argument in arguments {
                    self.check_length_capacity_operand(argument, source, iota)?;
                }
                let (ty, value) = self.eval_constant_expression(whole, source, iota)?;
                return Ok(CheckedOperand {
                    ty,
                    constant: Some(value),
                    contains_call_or_receive: false,
                });
            }
            if let Some(function) = self.functions.get(name) {
                self.check_call_arguments(&function.signature, arguments, spread, source, iota)?;
                let [result] = function.signature.results.as_slice() else {
                    return Err(Diagnostic::semantic(
                        "len/cap operand call must produce exactly one value",
                        source,
                    ));
                };
                return Ok(CheckedOperand {
                    ty: result.clone(),
                    constant: None,
                    contains_call_or_receive: true,
                });
            }
        }
        Err(Diagnostic::unsupported(
            "this len/cap call operand has no check-only semantic representation",
            source,
        ))
    }

    fn check_call_arguments(
        &self,
        signature: &Signature,
        arguments: &[ExprSyntax],
        spread: bool,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<(), Diagnostic> {
        if spread || signature.variadic || arguments.len() != signature.params.len() {
            return Err(Diagnostic::unsupported(
                "check-only len/cap calls currently require fixed ordinary arguments",
                source,
            ));
        }
        for (argument, expected) in arguments.iter().zip(&signature.params) {
            let checked = self.check_length_capacity_operand(argument, source, iota)?;
            if !is_assignable(&checked.ty, expected) {
                return Err(Diagnostic::semantic(
                    format!("cannot use {:?} as {expected:?}", checked.ty),
                    source,
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_length_capacity_composite(
        &self,
        ty: &ExprSyntax,
        elements: &[ExprSyntax],
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<CheckedOperand, Diagnostic> {
        let ty = self.lower_length_capacity_type(ty, source, iota)?;
        let Ty::Array(length, element) = ty.underlying() else {
            return Err(Diagnostic::unsupported(
                "check-only len/cap composite operands currently require an array type",
                source,
            ));
        };
        let length = *length;
        let element = element.as_ref().clone();
        let mut next_index = 0_u64;
        let mut contains_call_or_receive = false;
        let mut initialized = std::collections::BTreeSet::new();
        for syntax in elements {
            let (index, value) = match &syntax.kind {
                ExprSyntaxKind::KeyValue { key, value } => {
                    let checked_key = self.check_length_capacity_operand(key, source, iota)?;
                    if checked_key.constant.is_none() {
                        return Err(Diagnostic::semantic(
                            "array literal index must be an integer constant",
                            source,
                        ));
                    }
                    let (_, key) = self.eval_constant_expression(key, source, iota)?;
                    let ConstValue::Int(key) = key else {
                        return Err(Diagnostic::semantic(
                            "array literal index must be an integer constant",
                            source,
                        ));
                    };
                    let index = key.parse::<u64>().map_err(|_| {
                        Diagnostic::semantic("array literal index is outside u64", source)
                    })?;
                    (index, value.as_ref())
                }
                _ => (next_index, syntax),
            };
            if index >= length {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is outside length {length}"),
                    source,
                ));
            }
            if !initialized.insert(index) {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is initialized more than once"),
                    source,
                ));
            }
            next_index = index
                .checked_add(1)
                .ok_or_else(|| Diagnostic::semantic("array literal index overflow", source))?;
            let checked = if self.is_predeclared_nil_identifier(value) && is_nilable(&element) {
                CheckedOperand {
                    ty: element.clone(),
                    constant: None,
                    contains_call_or_receive: false,
                }
            } else {
                self.check_length_capacity_operand(value, source, iota)?
            };
            if !is_assignable(&checked.ty, &element)
                || checked
                    .constant
                    .as_ref()
                    .is_some_and(|constant| !constant.is_representable_as(&element))
            {
                return Err(Diagnostic::semantic(
                    format!("cannot use {:?} as array element {element:?}", checked.ty),
                    source,
                ));
            }
            contains_call_or_receive |= checked.contains_call_or_receive;
        }
        Ok(CheckedOperand {
            ty,
            constant: None,
            contains_call_or_receive,
        })
    }

    fn is_type_expression(&self, expression: &ExprSyntax) -> bool {
        match &expression.kind {
            ExprSyntaxKind::Paren(inner) => self.is_type_expression(inner),
            ExprSyntaxKind::Ident(identifier) => {
                self.type_aliases.contains_key(identifier.name.as_ref())
                    || ((super::super::type_lowering::predeclared_constant_type(&identifier.name)
                        .is_some()
                        || matches!(identifier.name.as_ref(), "any" | "error"))
                        && self.resolves_to_predeclared(identifier.name.as_ref()))
            }
            ExprSyntaxKind::ArrayType { .. }
            | ExprSyntaxKind::MapType { .. }
            | ExprSyntaxKind::ChannelType { .. }
            | ExprSyntaxKind::StructType { .. }
            | ExprSyntaxKind::InterfaceType { .. }
            | ExprSyntaxKind::FunctionType { .. }
            | ExprSyntaxKind::Unary {
                token: Token::MUL, ..
            } => true,
            ExprSyntaxKind::Index { base, .. } | ExprSyntaxKind::IndexList { base, .. } => {
                matches!(
                    &base.kind,
                    ExprSyntaxKind::Ident(identifier)
                        if self.generic_types.contains_key(identifier.name.as_ref())
                )
            }
            _ => false,
        }
    }

    fn lower_length_capacity_type(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<Ty, Diagnostic> {
        self.check_length_capacity_type_constants(expression, source, iota)?;
        super::super::lower_type_with_constant_lookup(
            expression,
            &self.type_aliases,
            &|name| {
                self.lookup_local_constant(name)
                    .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    .or_else(|| {
                        self.constants
                            .get(name)
                            .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    })
                    .or_else(|| {
                        (name == "iota" && self.resolves_to_predeclared(name))
                            .then_some(iota)
                            .flatten()
                            .map(|value| {
                                (
                                    Ty::Untyped(UntypedTy::Int),
                                    ConstValue::Int(value.to_string()),
                                )
                            })
                    })
            },
            source,
        )
    }

    fn check_length_capacity_type_constants(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<(), Diagnostic> {
        match &expression.kind {
            ExprSyntaxKind::Paren(inner)
            | ExprSyntaxKind::Unary {
                token: Token::MUL,
                expression: inner,
            }
            | ExprSyntaxKind::ChannelType { element: inner, .. } => {
                self.check_length_capacity_type_constants(inner, source, iota)
            }
            ExprSyntaxKind::ArrayType { length, element } => {
                if let Some(length) = length {
                    let checked = self.check_length_capacity_operand(length, source, iota)?;
                    if checked.constant.is_none() {
                        return Err(Diagnostic::semantic(
                            "array length must be an integer constant",
                            source,
                        ));
                    }
                }
                self.check_length_capacity_type_constants(element, source, iota)
            }
            ExprSyntaxKind::MapType { key, value } => {
                self.check_length_capacity_type_constants(key, source, iota)?;
                self.check_length_capacity_type_constants(value, source, iota)
            }
            ExprSyntaxKind::StructType { fields }
            | ExprSyntaxKind::InterfaceType { methods: fields } => {
                for field in fields.fields.iter() {
                    if let Some(ty) = &field.ty {
                        self.check_length_capacity_type_constants(ty, source, iota)?;
                    }
                }
                Ok(())
            }
            ExprSyntaxKind::FunctionType {
                params, results, ..
            } => {
                for field in params
                    .fields
                    .iter()
                    .chain(results.iter().flat_map(|fields| fields.fields.iter()))
                {
                    if let Some(ty) = &field.ty {
                        self.check_length_capacity_type_constants(ty, source, iota)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn is_predeclared_constant_builtin(&self, name: &str) -> bool {
        matches!(
            name,
            "len" | "cap" | "min" | "max" | "complex" | "real" | "imag"
        ) && self.resolves_to_predeclared(name)
    }

    pub(in crate::compiler::semantic) fn resolves_to_predeclared(&self, name: &str) -> bool {
        self.lookup_local(name).is_none()
            && self.lookup_closure(name).is_none()
            && self.lookup_local_constant(name).is_none()
            && !self.constants.contains_key(name)
            && !self.variables.contains_key(name)
            && !self.functions.contains_key(name)
            && !self.generic_functions.contains_key(name)
            && !self.type_aliases.contains_key(name)
            && !self.generic_types.contains_key(name)
            && !self.package_imports.contains(name)
            && !self.intrinsic_packages.contains(name)
            && !self
                .qualified_functions
                .keys()
                .any(|(package, _)| package == name)
            && !self
                .qualified_constants
                .keys()
                .any(|(package, _)| package == name)
            && !self
                .qualified_variables
                .keys()
                .any(|(package, _)| package == name)
    }
}
