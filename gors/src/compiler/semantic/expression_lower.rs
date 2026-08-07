//! Typed expression and direct-call lowering over owned structural syntax.

use crate::token::Token;

use super::FunctionLowerer;
use super::conversions::is_predeclared_conversion_name;
use super::expressions::*;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ComplexTy, ConstValue, IntTy, Ty, UintTy, UntypedTy};

impl FunctionLowerer {
    pub(super) fn local_expr(&self, node: NodeId, local: LocalId, ty: Ty) -> hir::Expr {
        hir::Expr {
            node,
            kind: hir::ExprKind::Local(local),
            ty,
            category: hir::ValueCategory::Place,
            effects: hir::Effects {
                may_read: true,
                ..hir::Effects::default()
            },
            source: SourceRef::node(node),
        }
    }

    pub(super) fn lower_expr(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if let Some(expected) = expected
            && is_nil_identifier(expr)
        {
            let node = self.alloc_node(expr.source)?;
            return self.zero_value_expr(node, SourceRef::node(node), expected.clone());
        }
        if let Some(expected) = expected
            && matches!(expected.underlying(), Ty::Interface(_))
        {
            let value = self.lower_expr_inner(expr, None, false)?;
            return self.coerce_interface_value(value, expected, expr.source);
        }
        self.lower_expr_inner(expr, expected, false)
    }

    pub(super) fn lower_expr_inner(
        &mut self,
        expr: &ExprSyntax,
        expected: Option<&Ty>,
        allow_discarded_call_result: bool,
    ) -> Result<hir::Expr, Diagnostic> {
        let node = self.alloc_node(expr.source)?;
        let source = SourceRef::node(node);
        let mut lowered = match &expr.kind {
            ExprSyntaxKind::Literal { .. } => {
                let (ty, value) = self.eval_constant_expression(expr, source, 0)?;
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Constant(value),
                    ty,
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    source,
                }
            }
            ExprSyntaxKind::Ident(ident) => {
                let name = ident.name.as_ref();
                if self.lookup_closure(name).is_some() {
                    return Err(Diagnostic::unsupported(
                        format!(
                            "local function {name} is non-escaping and can only be called directly"
                        ),
                        source,
                    ));
                } else if let Some(local) = self.lookup_local(name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    self.local_expr(node, local, ty)
                } else if let Some(constant) = self.lookup_local_constant(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(constant.value),
                        ty: constant.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if let Some(constant) = self.constants.get(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalConstant(constant.id, constant.value),
                        ty: constant.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if let Some(variable) = self.variables.get(name).cloned() {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::GlobalVariable(variable.id, variable.value),
                        ty: variable.ty,
                        category: hir::ValueCategory::Value,
                        effects: hir::Effects {
                            may_read: true,
                            ..hir::Effects::default()
                        },
                        source,
                    }
                } else if self.functions.contains_key(name)
                    || self.generic_functions.contains_key(name)
                {
                    return Err(Diagnostic::unsupported(
                        format!("function values for {name} are not yet supported"),
                        source,
                    ));
                } else if matches!(name, "true" | "false") {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(ConstValue::Bool(name == "true")),
                        ty: Ty::Untyped(UntypedTy::Bool),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else if matches!(name, "print" | "println") {
                    return Err(Diagnostic::unsupported(
                        format!("builtin value {name} is not implemented"),
                        source,
                    ));
                } else {
                    return Err(Diagnostic::semantic(
                        format!("undefined identifier {name}"),
                        source,
                    ));
                }
            }
            ExprSyntaxKind::Paren(expression) => {
                return self.lower_expr_inner(expression, expected, allow_discarded_call_result);
            }
            ExprSyntaxKind::Unary { token, expression } => {
                if *token == Token::ARROW {
                    return self.lower_channel_receive(expression, false, node, source, expected);
                }
                if *token == Token::AND {
                    return self.lower_address_of_local(expression, node, source, expected);
                }
                if *token == Token::MUL {
                    return self.lower_pointer_deref(expression, node, source, expected);
                }
                let mut operand = self.lower_expr(expression, expected)?;
                let operand_ty = operand.ty.default_typed();
                ensure_bootstrap_value_type(&operand_ty, source)?;
                let operator_ty = operand_ty.underlying();
                let op = match *token {
                    Token::ADD if matches!(operator_ty, Ty::Int(_) | Ty::Uint(_)) => {
                        hir::UnaryOp::Positive
                    }
                    Token::ADD
                        if matches!(
                            operator_ty,
                            Ty::Float(_) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Positive
                    }
                    Token::SUB if matches!(operator_ty, Ty::Int(_) | Ty::Uint(_)) => {
                        hir::UnaryOp::Negative
                    }
                    Token::SUB
                        if matches!(
                            operator_ty,
                            Ty::Float(_) | Ty::Complex(ComplexTy::Complex128)
                        ) =>
                    {
                        hir::UnaryOp::Negative
                    }
                    Token::NOT if is_bool(&operand_ty) => hir::UnaryOp::Not,
                    Token::XOR if matches!(operator_ty, Ty::Int(_) | Ty::Uint(_)) => {
                        hir::UnaryOp::BitNot
                    }
                    _ => {
                        return Err(Diagnostic::semantic(
                            format!("invalid unary {token:?} operand {:?}", operand.ty),
                            source,
                        ));
                    }
                };
                if let Some(value) = expr_constant(&operand)
                    .map(|value| fold_constant_unary(op, value, &operand.ty, source))
                    .transpose()?
                    .flatten()
                {
                    let value = normalize_constant_for_type(value, &operand.ty, source)?;
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: operand.ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else {
                    coerce_expr(&mut operand, &operand_ty, source)?;
                    let effects = operand.effects;
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Unary {
                            op,
                            operand: Box::new(operand),
                        },
                        ty: operand_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        source,
                    }
                }
            }
            ExprSyntaxKind::Binary { left, token, right } => {
                if matches!(token, Token::EQL | Token::NEQ) {
                    let map = if is_nil_identifier(left) {
                        Some(right.as_ref())
                    } else if is_nil_identifier(right) {
                        Some(left.as_ref())
                    } else {
                        None
                    };
                    if let Some(map) = map {
                        return self.lower_nil_comparison(
                            map,
                            *token == Token::EQL,
                            node,
                            source,
                            expected,
                        );
                    }
                }
                let left_syntax_source = left.source;
                let right_syntax_source = right.source;
                let mut left = self.lower_expr(left, None)?;
                let mut right = self.lower_expr(right, None)?;
                if matches!(token, Token::EQL | Token::NEQ)
                    && (left.ty.bootstrap_i64_struct_pointer_fields().is_some()
                        || right.ty.bootstrap_i64_struct_pointer_fields().is_some())
                {
                    return self.lower_struct_pointer_comparison(
                        left,
                        right,
                        *token == Token::EQL,
                        node,
                        source,
                        expected,
                    );
                }
                if matches!(token, Token::EQL | Token::NEQ)
                    && (matches!(left.ty.underlying(), Ty::Interface(_))
                        || matches!(right.ty.underlying(), Ty::Interface(_)))
                {
                    return self.lower_interface_comparison(
                        left,
                        left_syntax_source,
                        right,
                        right_syntax_source,
                        *token == Token::EQL,
                        node,
                        source,
                        expected,
                    );
                }
                let op = lower_binary_op(*token).ok_or_else(|| {
                    Diagnostic::unsupported(
                        format!("binary operator {token:?} is not implemented"),
                        source,
                    )
                })?;
                if matches!(op, hir::BinaryOp::Shl | hir::BinaryOp::Shr)
                    && matches!(left.ty, Ty::Untyped(_))
                    && (right.ty.is_integer() || matches!(right.ty, Ty::Untyped(_)))
                    && let (Some(left_value), Some(right_value)) =
                        (expr_constant(&left), expr_constant(&right))
                {
                    let value = fold_untyped_constant_shift(op, left_value, right_value, source)?;
                    let mut lowered = hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty: Ty::Untyped(UntypedTy::Int),
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    };
                    if let Some(expected) = expected {
                        coerce_expr(&mut lowered, expected, source)?;
                    }
                    return Ok(lowered);
                }
                let comparison = matches!(
                    op,
                    hir::BinaryOp::Equal
                        | hir::BinaryOp::NotEqual
                        | hir::BinaryOp::Less
                        | hir::BinaryOp::LessEqual
                        | hir::BinaryOp::Greater
                        | hir::BinaryOp::GreaterEqual
                );
                let logical = matches!(op, hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr);
                let operand_ty = common_operand_type(&left.ty, &right.ty).ok_or_else(|| {
                    Diagnostic::semantic(
                        format!(
                            "incompatible binary operands {:?} and {:?}",
                            left.ty, right.ty
                        ),
                        source,
                    )
                })?;
                // A folded operation on two untyped operands stays an untyped
                // constant of the merged kind, so a later conversion site can
                // still adapt the exact value. Fold before default-type
                // materialization: an intermediate Go constant need not fit
                // float64 when the final exact result does.
                let untyped_operand_ty = exact_common_operand_type(&left.ty, &right.ty)
                    .filter(|ty| matches!(ty, Ty::Untyped(_)));
                if expr_constant(&left).is_some() && expr_constant(&right).is_some() {
                    super::validate_constant_binary_operator(op, &operand_ty, source)?;
                } else {
                    validate_binary_operator(op, &operand_ty, source)?;
                }
                let folded_untyped = if untyped_operand_ty.is_some() {
                    expr_constant(&left)
                        .zip(expr_constant(&right))
                        .map(|(left, right)| fold_constant_binary(op, left, right, source))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
                if folded_untyped.is_none() {
                    coerce_expr(&mut left, &operand_ty, source)?;
                    coerce_expr(&mut right, &operand_ty, source)?;
                }
                let mut effects = left.effects.union(right.effects);
                if matches!(
                    op,
                    hir::BinaryOp::Div
                        | hir::BinaryOp::Rem
                        | hir::BinaryOp::Shl
                        | hir::BinaryOp::Shr
                ) {
                    effects.may_panic = true;
                }
                if op == hir::BinaryOp::Add && operand_ty == Ty::String {
                    effects.may_allocate = true;
                }
                let result_ty = if comparison || logical {
                    Ty::Bool
                } else {
                    operand_ty
                };
                let folded = match folded_untyped {
                    Some(value) => Some(value),
                    None => expr_constant(&left)
                        .zip(expr_constant(&right))
                        .map(|(left, right)| fold_constant_binary(op, left, right, source))
                        .transpose()?
                        .flatten(),
                };
                if let Some(value) = folded {
                    let ty = if comparison || logical {
                        result_ty
                    } else {
                        untyped_operand_ty.unwrap_or(result_ty)
                    };
                    let value = normalize_constant_for_type(value, &ty, source)?;
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Constant(value),
                        ty,
                        category: hir::ValueCategory::Constant,
                        effects: hir::Effects::default(),
                        source,
                    }
                } else {
                    hir::Expr {
                        node,
                        kind: hir::ExprKind::Binary {
                            op,
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                        ty: result_ty,
                        category: hir::ValueCategory::Value,
                        effects,
                        source,
                    }
                }
            }
            ExprSyntaxKind::Call {
                callee,
                arguments,
                spread,
            } => {
                if let ExprSyntaxKind::Index { base, index } = &callee.kind
                    && arguments.is_empty()
                    && !spread
                    && !matches!(
                        &base.kind,
                        ExprSyntaxKind::Ident(name)
                            if self.generic_functions.contains_key(name.name.as_ref())
                    )
                {
                    let slice = self.lower_expr(base, None)?;
                    if let Some(function) = slice.ty.snapshot_function_slice_element() {
                        let result_ty =
                            function
                                .snapshot_function_result()
                                .cloned()
                                .ok_or_else(|| {
                                    Diagnostic::backend(
                                        "snapshot function slice lost its result type",
                                    )
                                })?;
                        let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                        let effects = slice_runtime_effects(&[&slice, &index], false, false, true);
                        let mut call = hir::Expr {
                            node,
                            kind: hir::ExprKind::Call {
                                callee: hir::Callee::Builtin(
                                    hir::Builtin::SnapshotFunctionSliceCall,
                                ),
                                args: vec![slice, index],
                            },
                            ty: result_ty,
                            category: hir::ValueCategory::Value,
                            effects,
                            source,
                        };
                        if let Some(expected) = expected {
                            coerce_expr(&mut call, expected, source)?;
                        }
                        return Ok(call);
                    }
                }
                let explicit_generic = match &callee.kind {
                    ExprSyntaxKind::Index { base, index } => {
                        let ExprSyntaxKind::Ident(name) = &base.kind else {
                            return self.lower_conversion_call(
                                callee, arguments, *spread, node, source, expected,
                            );
                        };
                        self.generic_functions
                            .contains_key(name.name.as_ref())
                            .then_some((name.name.as_ref(), std::slice::from_ref(index.as_ref())))
                    }
                    ExprSyntaxKind::IndexList { base, indices } => {
                        let ExprSyntaxKind::Ident(name) = &base.kind else {
                            return self.lower_conversion_call(
                                callee, arguments, *spread, node, source, expected,
                            );
                        };
                        self.generic_functions
                            .contains_key(name.name.as_ref())
                            .then_some((name.name.as_ref(), indices.as_ref()))
                    }
                    _ => None,
                };
                if let Some((name, type_arguments)) = explicit_generic {
                    return self.lower_explicit_generic_function_call(
                        name,
                        type_arguments,
                        arguments,
                        *spread,
                        node,
                        expr.source,
                        source,
                        expected,
                        allow_discarded_call_result,
                    );
                }
                if let ExprSyntaxKind::Selector { base, member } = &callee.kind {
                    return self.lower_selector_call(
                        base,
                        member,
                        arguments,
                        *spread,
                        node,
                        source,
                        expected,
                        allow_discarded_call_result,
                    );
                }
                let ExprSyntaxKind::Ident(callee_ident) = &callee.kind else {
                    return self
                        .lower_conversion_call(callee, arguments, *spread, node, source, expected);
                };
                let name = callee_ident.name.as_ref();
                if name == "make" {
                    return self
                        .lower_make_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "new" {
                    return self.lower_new_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "recover" {
                    return self
                        .lower_recover_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "len" {
                    return self.lower_len_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "cap" {
                    return self.lower_channel_cap_builtin_call(
                        arguments, *spread, node, source, expected,
                    );
                }
                if name == "close" {
                    return self.lower_channel_close_builtin_call(
                        arguments, *spread, node, source, expected,
                    );
                }
                if name == "clear" {
                    return self
                        .lower_clear_builtin_call(arguments, *spread, node, source, expected);
                }
                if name == "delete" {
                    return self
                        .lower_delete_builtin_call(arguments, *spread, node, source, expected);
                }
                if matches!(name, "append" | "copy") {
                    return self.lower_slice_builtin_call(
                        name, arguments, *spread, node, source, expected,
                    );
                }
                if matches!(name, "min" | "max" | "complex" | "real" | "imag") {
                    return self.lower_numeric_builtin_call(
                        name, arguments, *spread, node, source, expected,
                    );
                }
                if self.type_aliases.contains_key(name) || is_predeclared_conversion_name(name) {
                    return self
                        .lower_conversion_call(callee, arguments, *spread, node, source, expected);
                }
                if self.generic_functions.contains_key(name) {
                    return self.lower_generic_function_call(
                        name,
                        arguments,
                        *spread,
                        node,
                        expr.source,
                        source,
                        expected,
                        allow_discarded_call_result,
                    );
                }
                let (callee, params, results, variadic) =
                    if let Some(id) = self.lookup_closure(name) {
                        let closure = self.closures.get(id.index() as usize).ok_or_else(|| {
                            Diagnostic::backend(format!("unknown local function {name}"))
                        })?;
                        (
                            hir::Callee::Closure(id),
                            closure.signature.params.clone(),
                            closure.signature.results.clone(),
                            closure.signature.variadic,
                        )
                    } else if self.lookup_local(name).is_some() {
                        return Err(Diagnostic::unsupported(
                            format!("calling the function value {name} is not yet supported"),
                            source,
                        ));
                    } else if let Some(symbol) = self.functions.get(name).cloned() {
                        (
                            hir::Callee::Function(symbol.id),
                            symbol.signature.params,
                            symbol.signature.results,
                            symbol.signature.variadic,
                        )
                    } else if self.lookup_local_constant(name).is_some()
                        || self.constants.contains_key(name)
                    {
                        return Err(Diagnostic::semantic(
                            format!("constant {name} is not callable"),
                            source,
                        ));
                    } else {
                        match name {
                            "print" => (
                                hir::Callee::Builtin(hir::Builtin::Print),
                                vec![],
                                vec![],
                                false,
                            ),
                            "println" => (
                                hir::Callee::Builtin(hir::Builtin::Println),
                                vec![],
                                vec![],
                                false,
                            ),
                            "panic" => (
                                hir::Callee::Builtin(hir::Builtin::Panic),
                                vec![],
                                vec![],
                                false,
                            ),
                            name => {
                                return Err(Diagnostic::semantic(
                                    format!("undefined function {name}"),
                                    source,
                                ));
                            }
                        }
                    };
                match callee {
                    hir::Callee::Builtin(hir::Builtin::Panic) if arguments.len() != 1 => {
                        return Err(Diagnostic::semantic(
                            format!(
                                "call to panic has {} arguments; expected 1",
                                arguments.len()
                            ),
                            source,
                        ));
                    }
                    hir::Callee::Builtin(_) if *spread => {
                        return Err(Diagnostic::semantic(
                            "... is not valid for this built-in call",
                            source,
                        ));
                    }
                    _ => {}
                }
                let args = if callee == hir::Callee::Builtin(hir::Builtin::Panic) {
                    let any = Ty::Interface(Vec::new());
                    arguments
                        .iter()
                        .map(|argument| self.lower_expr(argument, Some(&any)))
                        .collect::<Result<Vec<_>, _>>()?
                } else if matches!(callee, hir::Callee::Function(_) | hir::Callee::Closure(_)) {
                    self.lower_call_arguments(
                        arguments,
                        &params,
                        variadic,
                        *spread,
                        expr.source,
                        source,
                        "function",
                    )?
                } else {
                    arguments
                        .iter()
                        .map(|argument| {
                            self.lower_expr(argument, None).and_then(|expression| {
                                let expression_source = expression.source;
                                default_expr_type(expression, expression_source)
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                let ty = match results.as_slice() {
                    [] => Ty::Unit,
                    [single] => single.clone(),
                    many => Ty::Tuple(many.to_vec()),
                };
                if ty == Ty::Unit && !allow_discarded_call_result {
                    return Err(Diagnostic::unsupported(
                        "a no-result call cannot be used as a value",
                        source,
                    ));
                }
                let effects = args.iter().fold(
                    hir::Effects {
                        may_read: false,
                        may_call: true,
                        may_allocate: true,
                        may_block: true,
                        may_panic: true,
                        may_write: true,
                    },
                    |effects, argument| effects.union(argument.effects),
                );
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call { callee, args },
                    ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::FunctionLiteral { .. } => {
                return Err(Diagnostic::unsupported(
                    "function literals currently require a non-escaping short declaration",
                    source,
                ));
            }
            ExprSyntaxKind::Selector { base, member } => {
                self.lower_selector(base, member, node, source)?
            }
            ExprSyntaxKind::TypeAssert { value, asserted } => self.lower_interface_type_assertion(
                value,
                asserted.as_deref(),
                false,
                node,
                source,
            )?,
            ExprSyntaxKind::CompositeLiteral { ty, elements } => self.lower_composite_literal(
                ty.as_deref(),
                elements,
                node,
                expr.source,
                source,
                expected,
            )?,
            ExprSyntaxKind::Index { base, index } => {
                let mut base = self.lower_expr(base, None)?;
                if base.ty == Ty::Untyped(UntypedTy::String) {
                    // An untyped constant string operand of an index expression
                    // assumes its default type; the element read stays a
                    // non-constant byte.
                    let base_source = base.source;
                    coerce_expr(&mut base, &Ty::String, base_source)?;
                }
                if matches!(base.ty.underlying(), Ty::Map(_, _)) {
                    return self.lower_map_index(base, index, node, source, expected);
                }
                if matches!(base.ty.underlying(), Ty::Array(_, _)) {
                    return self.lower_array_index(base, index, node, source, expected);
                }
                let (builtin, element_ty) = match base.ty.underlying() {
                    Ty::String => (hir::Builtin::StringIndex, Ty::Uint(UintTy::Uint8)),
                    Ty::Slice(element)
                        if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) =>
                    {
                        (hir::Builtin::SliceI64Index, element.as_ref().clone())
                    }
                    Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8) => {
                        (hir::Builtin::SliceU8Index, element.as_ref().clone())
                    }
                    Ty::Slice(element) if element.underlying() == &Ty::Bool => {
                        (hir::Builtin::SliceBoolIndex, element.as_ref().clone())
                    }
                    Ty::Slice(element) if element.underlying() == &Ty::String => {
                        (hir::Builtin::SliceGoStringIndex, element.as_ref().clone())
                    }
                    Ty::Slice(element) if element.uses_interface_aggregate_representation() => {
                        let stored_ty = element.as_ref().clone();
                        let element_ty = self.expand_named_ref(&stored_ty)?;
                        let type_identity = stored_ty.dynamic_type_identity().ok_or_else(|| {
                            Diagnostic::backend(
                                "aggregate slice element omitted its dynamic type identity",
                            )
                        })?;
                        let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                        let effects = slice_runtime_effects(&[&base, &index], false, false, true);
                        return Ok(hir::Expr {
                            node,
                            kind: hir::ExprKind::AggregateSliceIndex {
                                slice: Box::new(base),
                                index: Box::new(index),
                                type_identity,
                            },
                            ty: element_ty,
                            category: hir::ValueCategory::Value,
                            effects,
                            source,
                        });
                    }
                    ty => {
                        return Err(Diagnostic::unsupported(
                            format!("indexing is not yet implemented for {ty:?}"),
                            source,
                        ));
                    }
                };
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                let effects = slice_runtime_effects(&[&base, &index], false, false, true);
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call {
                        callee: hir::Callee::Builtin(builtin),
                        args: vec![base, index],
                    },
                    ty: element_ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::IndexList { .. } => {
                return Err(Diagnostic::unsupported(
                    "generic instantiation expressions with multiple type arguments are not yet executable",
                    source,
                ));
            }
            ExprSyntaxKind::Slice {
                base,
                low,
                high,
                max,
            } => {
                let base = self.lower_expr(base, None)?;
                let base_ty = base.ty.clone();
                let low_bound = self.lower_optional_slice_bound(low.as_deref(), expr.source)?;
                let high_bound = self.lower_optional_slice_bound(high.as_deref(), expr.source)?;
                // A missing low bound is the constant zero. Missing high and
                // max bounds depend on runtime length or capacity, so they
                // stay unknown here and keep their runtime bounds checks.
                let low_constant = low
                    .as_deref()
                    .map_or(Some(0), |_| constant_index_value(&low_bound));
                let high_constant = high
                    .as_deref()
                    .and_then(|_| constant_index_value(&high_bound));
                if let (Some(low_value), Some(high_value)) = (low_constant, high_constant)
                    && low_value > high_value
                {
                    return Err(Diagnostic::semantic(
                        format!("invalid slice indices: {high_value} < {low_value}"),
                        source,
                    ));
                }
                let (builtin, ty, args) = match base.ty.underlying() {
                    Ty::String => {
                        if max.is_some() {
                            return Err(Diagnostic::semantic(
                                "three-index slicing requires a slice value",
                                source,
                            ));
                        }
                        (
                            hir::Builtin::StringRange,
                            Ty::String,
                            vec![base, low_bound, high_bound],
                        )
                    }
                    Ty::Slice(element)
                        if matches!(
                            element.underlying(),
                            Ty::Int(IntTy::Int | IntTy::Int32)
                                | Ty::Uint(UintTy::Uint8)
                                | Ty::String
                        ) =>
                    {
                        let builtin =
                            if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32)) {
                                hir::Builtin::SliceI64Range
                            } else if element.underlying() == &Ty::Uint(UintTy::Uint8) {
                                hir::Builtin::SliceU8Range
                            } else {
                                hir::Builtin::SliceGoStringRange
                            };
                        let max_bound =
                            self.lower_optional_slice_bound(max.as_deref(), expr.source)?;
                        if let Some(max_value) = max
                            .as_deref()
                            .and_then(|_| constant_index_value(&max_bound))
                        {
                            if let Some(high_value) = high_constant
                                && high_value > max_value
                            {
                                return Err(Diagnostic::semantic(
                                    format!("invalid slice indices: {max_value} < {high_value}"),
                                    source,
                                ));
                            }
                            if let Some(low_value) = low_constant
                                && low_value > max_value
                            {
                                return Err(Diagnostic::semantic(
                                    format!("invalid slice indices: {max_value} < {low_value}"),
                                    source,
                                ));
                            }
                        }
                        (
                            builtin,
                            base_ty,
                            vec![base, low_bound, high_bound, max_bound],
                        )
                    }
                    ty => {
                        return Err(Diagnostic::unsupported(
                            format!("slicing is not yet implemented for {ty:?}"),
                            source,
                        ));
                    }
                };
                let effects =
                    slice_runtime_effects(&args.iter().collect::<Vec<_>>(), false, false, true);
                hir::Expr {
                    node,
                    kind: hir::ExprKind::Call {
                        callee: hir::Callee::Builtin(builtin),
                        args,
                    },
                    ty,
                    category: hir::ValueCategory::Value,
                    effects,
                    source,
                }
            }
            ExprSyntaxKind::ArrayType { .. }
            | ExprSyntaxKind::MapType { .. }
            | ExprSyntaxKind::ChannelType { .. }
            | ExprSyntaxKind::FunctionType { .. }
            | ExprSyntaxKind::StructType { .. }
            | ExprSyntaxKind::InterfaceType { .. } => {
                return Err(Diagnostic::semantic(
                    "a type is not a value expression",
                    source,
                ));
            }
            ExprSyntaxKind::KeyValue { .. } => {
                return Err(Diagnostic::semantic(
                    "key: value syntax requires a composite literal",
                    source,
                ));
            }
            ExprSyntaxKind::Unsupported(kind) => {
                return Err(Diagnostic::unsupported(
                    format!("expression {kind} is not yet supported"),
                    source,
                ));
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut lowered, expected, source)?;
        }
        Ok(lowered)
    }

    pub(super) fn lower_optional_slice_bound(
        &mut self,
        bound: Option<&ExprSyntax>,
        syntax_source: crate::compiler::syntax::SyntaxSource,
    ) -> Result<hir::Expr, Diagnostic> {
        if let Some(bound) = bound {
            return self.lower_expr(bound, Some(&Ty::Int(IntTy::Int)));
        }
        let node = self.alloc_node(syntax_source)?;
        Ok(hir::Expr {
            node,
            kind: hir::ExprKind::Constant(ConstValue::Int("-1".into())),
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(node),
        })
    }
}

/// Exact integer value of an already-lowered constant expression.
///
/// Returns `None` for runtime values, so callers only reject when the Go spec
/// mandates a compile-time failure and keep runtime bounds panics otherwise.
pub(super) fn constant_index_value(expression: &hir::Expr) -> Option<i64> {
    let (hir::ExprKind::Constant(value) | hir::ExprKind::GlobalConstant(_, value)) =
        &expression.kind
    else {
        return None;
    };
    let ConstValue::Int(text) = value else {
        return None;
    };
    text.parse::<i64>().ok()
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == "nil"
    )
}

pub(super) fn slice_runtime_effects(
    arguments: &[&hir::Expr],
    writes: bool,
    allocates: bool,
    panics: bool,
) -> hir::Effects {
    arguments.iter().fold(
        hir::Effects {
            may_call: true,
            may_allocate: allocates,
            may_panic: panics,
            may_write: writes,
            ..hir::Effects::default()
        },
        |effects, argument| effects.union(argument.effects),
    )
}
