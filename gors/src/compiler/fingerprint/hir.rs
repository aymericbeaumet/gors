//! Canonical encoding of typed HIR.

use super::Fingerprint;
use super::encoder::{
    Encoder, const_value, def_id, hir_effects, local_id, node_id, signature, source_ref, ty,
};
use crate::compiler::hir;

/// Fingerprint a complete typed HIR file, including declaration order.
#[must_use]
pub fn hir_file(file: &hir::File) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-file");
    encode_file(&mut encoder, file);
    encoder.finish()
}

/// Fingerprint one typed HIR function independently of sibling declarations.
#[must_use]
pub fn hir_function(function: &hir::Function) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-function");
    encode_function(&mut encoder, function);
    encoder.finish()
}

/// Fingerprint one exact typed package constant independently of siblings.
#[must_use]
pub fn hir_constant(constant: &hir::Constant) -> Fingerprint {
    let mut encoder = Encoder::root(b"hir-constant");
    encode_constant(&mut encoder, constant);
    encoder.finish()
}

fn encode_file(encoder: &mut Encoder, file: &hir::File) {
    encoder.field(b"package", |encoder| encoder.string(&file.package));
    encoder.field(b"constants", |encoder| {
        encoder.sequence(&file.constants, encode_constant);
    });
    encoder.field(b"functions", |encoder| {
        encoder.sequence(&file.functions, encode_function);
    });
}

fn encode_constant(encoder: &mut Encoder, constant: &hir::Constant) {
    encoder.field(b"id", |encoder| def_id(encoder, constant.id));
    encoder.field(b"name", |encoder| encoder.string(&constant.name));
    encoder.field(b"type", |encoder| ty(encoder, &constant.ty));
    encoder.field(b"value", |encoder| const_value(encoder, &constant.value));
    encoder.field(b"source", |encoder| source_ref(encoder, constant.source));
}

fn encode_function(encoder: &mut Encoder, function: &hir::Function) {
    encoder.field(b"id", |encoder| def_id(encoder, function.id));
    encoder.field(b"node", |encoder| node_id(encoder, function.node));
    encoder.field(b"name", |encoder| encoder.string(&function.name));
    encoder.field(b"signature", |encoder| {
        signature(encoder, &function.signature);
    });
    encoder.field(b"parameters", |encoder| {
        encoder.sequence(&function.params, |encoder, id| local_id(encoder, *id));
    });
    encoder.field(b"named-results", |encoder| {
        encoder.sequence(&function.named_results, |encoder, result| {
            encoder.option(result.as_ref(), |encoder, id| local_id(encoder, *id));
        });
    });
    encoder.field(b"locals", |encoder| {
        encoder.sequence(&function.locals, encode_local);
    });
    encoder.field(b"body", |encoder| {
        encode_block(encoder, &function.body);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, function.source));
}

fn encode_local(encoder: &mut Encoder, local: &hir::Local) {
    encoder.field(b"id", |encoder| local_id(encoder, local.id));
    encoder.field(b"name", |encoder| {
        encoder.option(local.name.as_ref(), |encoder, name| encoder.string(name));
    });
    encoder.field(b"type", |encoder| ty(encoder, &local.ty));
    encoder.field(b"kind", |encoder| encode_local_kind(encoder, local.kind));
    encoder.field(b"source", |encoder| source_ref(encoder, local.source));
}

fn encode_local_kind(encoder: &mut Encoder, kind: hir::LocalKind) {
    encoder.variant(
        match kind {
            hir::LocalKind::Parameter => b"parameter",
            hir::LocalKind::NamedResult => b"named-result",
            hir::LocalKind::Variable => b"variable",
            hir::LocalKind::Temporary => b"temporary",
        },
        |_| {},
    );
}

fn encode_block(encoder: &mut Encoder, block: &hir::Block) {
    encoder.field(b"node", |encoder| node_id(encoder, block.node));
    encoder.field(b"statements", |encoder| {
        encoder.sequence(&block.stmts, encode_statement);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, block.source));
}

fn encode_statement(encoder: &mut Encoder, statement: &hir::Stmt) {
    encoder.field(b"node", |encoder| node_id(encoder, statement.node));
    encoder.field(b"kind", |encoder| {
        encode_statement_kind(encoder, &statement.kind);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, statement.source));
}

fn encode_statement_kind(encoder: &mut Encoder, kind: &hir::StmtKind) {
    match kind {
        hir::StmtKind::Let {
            destinations,
            values,
        } => encoder.variant(b"let", |encoder| {
            encode_places_and_values(encoder, destinations, values);
        }),
        hir::StmtKind::LetTuple {
            destinations,
            value,
        } => encoder.variant(b"let-tuple", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
        }),
        hir::StmtKind::Assign {
            destinations,
            op,
            values,
        } => encoder.variant(b"assign", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"operation", |encoder| encode_assign_op(encoder, *op));
            encoder.field(b"values", |encoder| {
                encoder.sequence(values, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
        }),
        hir::StmtKind::AssignTuple {
            destinations,
            value,
        } => encoder.variant(b"assign-tuple", |encoder| {
            encoder.field(b"destinations", |encoder| {
                encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
            });
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
        }),
        hir::StmtKind::Expr(expression) => {
            encoder.variant(b"expression", |encoder| {
                encode_expression(encoder, expression)
            });
        }
        hir::StmtKind::Defer {
            parameters,
            values,
            body,
        } => encoder.variant(b"defer", |encoder| {
            encoder.field(b"parameters", |encoder| {
                encoder.sequence(parameters, |encoder, parameter| {
                    local_id(encoder, *parameter);
                });
            });
            encoder.field(b"values", |encoder| {
                encoder.sequence(values, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
            encoder.field(b"body", |encoder| encode_block(encoder, body));
        }),
        hir::StmtKind::Return(values) => encoder.variant(b"return", |encoder| {
            encoder.sequence(values, |encoder, expression| {
                encode_expression(encoder, expression);
            });
        }),
        hir::StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => encoder.variant(b"if", |encoder| {
            encoder.field(b"init", |encoder| {
                encoder.option(init.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"condition", |encoder| {
                encode_expression(encoder, condition);
            });
            encoder.field(b"then", |encoder| {
                encode_block(encoder, then_block);
            });
            encoder.field(b"else", |encoder| {
                encoder.option(else_branch.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
        }),
        hir::StmtKind::For {
            label,
            init,
            condition,
            post,
            body,
        } => encoder.variant(b"for", |encoder| {
            encoder.field(b"label", |encoder| {
                encoder.option(label.as_ref(), |encoder, label| encoder.string(label));
            });
            encoder.field(b"init", |encoder| {
                encoder.option(init.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"condition", |encoder| {
                encoder.option(condition.as_ref(), |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
            encoder.field(b"post", |encoder| {
                encoder.option(post.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
            encoder.field(b"body", |encoder| {
                encode_block(encoder, body);
            });
        }),
        hir::StmtKind::Block(block) => {
            encoder.variant(b"block", |encoder| {
                encode_block(encoder, block);
            });
        }
        hir::StmtKind::Label { name, statement } => encoder.variant(b"label", |encoder| {
            encoder.field(b"name", |encoder| encoder.string(name));
            encoder.field(b"statement", |encoder| {
                encoder.option(statement.as_deref(), |encoder, statement| {
                    encode_statement(encoder, statement);
                });
            });
        }),
        hir::StmtKind::Goto(label) => encoder.variant(b"goto", |encoder| encoder.string(label)),
        hir::StmtKind::Break(label) => encoder.variant(b"break", |encoder| {
            encoder.option(label.as_ref(), |encoder, label| encoder.string(label));
        }),
        hir::StmtKind::Continue(label) => encoder.variant(b"continue", |encoder| {
            encoder.option(label.as_ref(), |encoder, label| encoder.string(label));
        }),
        hir::StmtKind::SliceAssign {
            slice,
            index,
            op,
            value,
        } => encoder.variant(b"slice-assign", |encoder| {
            encoder.field(b"slice", |encoder| encode_expression(encoder, slice));
            encoder.field(b"index", |encoder| encode_expression(encoder, index));
            encoder.field(b"operation", |encoder| encode_assign_op(encoder, *op));
            encoder.field(b"value", |encoder| encode_expression(encoder, value));
        }),
    }
}

fn encode_places_and_values(
    encoder: &mut Encoder,
    destinations: &[hir::Place],
    values: &[hir::Expr],
) {
    encoder.field(b"destinations", |encoder| {
        encoder.sequence(destinations, |encoder, place| encode_place(encoder, *place));
    });
    encoder.field(b"values", |encoder| {
        encoder.sequence(values, |encoder, expression| {
            encode_expression(encoder, expression);
        });
    });
}

fn encode_place(encoder: &mut Encoder, place: hir::Place) {
    match place {
        hir::Place::Local(id) => {
            encoder.variant(b"local", |encoder| local_id(encoder, id));
        }
        hir::Place::Discard => encoder.variant(b"discard", |_| {}),
    }
}

fn encode_assign_op(encoder: &mut Encoder, op: hir::AssignOp) {
    encoder.variant(
        match op {
            hir::AssignOp::Set => b"set",
            hir::AssignOp::Add => b"add",
            hir::AssignOp::Sub => b"subtract",
            hir::AssignOp::Mul => b"multiply",
            hir::AssignOp::Div => b"divide",
            hir::AssignOp::Rem => b"remainder",
            hir::AssignOp::BitAnd => b"bit-and",
            hir::AssignOp::BitOr => b"bit-or",
            hir::AssignOp::BitXor => b"bit-xor",
            hir::AssignOp::Shl => b"shift-left",
            hir::AssignOp::Shr => b"shift-right",
            hir::AssignOp::AndNot => b"and-not",
        },
        |_| {},
    );
}

fn encode_expression(encoder: &mut Encoder, expression: &hir::Expr) {
    encoder.field(b"node", |encoder| node_id(encoder, expression.node));
    encoder.field(b"kind", |encoder| {
        encode_expression_kind(encoder, &expression.kind);
    });
    encoder.field(b"type", |encoder| ty(encoder, &expression.ty));
    encoder.field(b"category", |encoder| {
        encode_value_category(encoder, expression.category);
    });
    encoder.field(b"effects", |encoder| {
        hir_effects(encoder, expression.effects);
    });
    encoder.field(b"source", |encoder| source_ref(encoder, expression.source));
}

fn encode_expression_kind(encoder: &mut Encoder, kind: &hir::ExprKind) {
    match kind {
        hir::ExprKind::Constant(value) => {
            encoder.variant(b"constant", |encoder| const_value(encoder, value));
        }
        hir::ExprKind::Local(id) => {
            encoder.variant(b"local", |encoder| local_id(encoder, *id));
        }
        hir::ExprKind::GlobalConstant(id, value) => {
            encoder.variant(b"global-constant", |encoder| {
                encoder.field(b"id", |encoder| def_id(encoder, *id));
                encoder.field(b"value", |encoder| const_value(encoder, value));
            });
        }
        hir::ExprKind::Binary { op, left, right } => {
            encoder.variant(b"binary", |encoder| {
                encoder.field(b"operation", |encoder| encode_binary_op(encoder, *op));
                encoder.field(b"left", |encoder| {
                    encode_expression(encoder, left);
                });
                encoder.field(b"right", |encoder| {
                    encode_expression(encoder, right);
                });
            });
        }
        hir::ExprKind::Unary { op, operand } => encoder.variant(b"unary", |encoder| {
            encoder.field(b"operation", |encoder| encode_unary_op(encoder, *op));
            encoder.field(b"operand", |encoder| {
                encode_expression(encoder, operand);
            });
        }),
        hir::ExprKind::Conversion { value } => encoder.variant(b"conversion", |encoder| {
            encode_expression(encoder, value);
        }),
        hir::ExprKind::SliceLiteralI64(elements) => {
            encoder.variant(b"slice-literal-i64", |encoder| {
                encoder.sequence(elements, |encoder, element| encoder.i64(*element));
            });
        }
        hir::ExprKind::SliceLiteralU8(elements) => {
            encoder.variant(b"slice-literal-u8", |encoder| encoder.blob(elements));
        }
        hir::ExprKind::Call { callee, args } => encoder.variant(b"call", |encoder| {
            encoder.field(b"callee", |encoder| encode_callee(encoder, *callee));
            encoder.field(b"arguments", |encoder| {
                encoder.sequence(args, |encoder, expression| {
                    encode_expression(encoder, expression);
                });
            });
        }),
    }
}

fn encode_callee(encoder: &mut Encoder, callee: hir::Callee) {
    match callee {
        hir::Callee::Function(id) => {
            encoder.variant(b"function", |encoder| def_id(encoder, id));
        }
        hir::Callee::Builtin(builtin) => {
            encoder.variant(b"builtin", |encoder| encode_builtin(encoder, builtin));
        }
    }
}

fn encode_builtin(encoder: &mut Encoder, builtin: hir::Builtin) {
    encoder.variant(
        match builtin {
            hir::Builtin::Print => b"print",
            hir::Builtin::Println => b"println",
            hir::Builtin::Panic => b"panic",
            hir::Builtin::SliceI64Index => b"slice-i64-index",
            hir::Builtin::SliceI64Range => b"slice-i64-range",
            hir::Builtin::SliceI64Set => b"slice-i64-set",
            hir::Builtin::SliceI64Make => b"slice-i64-make",
            hir::Builtin::SliceI64Len => b"slice-i64-len",
            hir::Builtin::SliceI64Cap => b"slice-i64-cap",
            hir::Builtin::SliceI64Append => b"slice-i64-append",
            hir::Builtin::SliceU8AppendSlice => b"slice-u8-append-slice",
            hir::Builtin::SliceU8AppendString => b"slice-u8-append-string",
            hir::Builtin::SliceU8CopyString => b"slice-u8-copy-string",
            hir::Builtin::SliceI64Clear => b"slice-i64-clear",
            hir::Builtin::StringFromSliceU8 => b"string-from-slice-u8",
        },
        |_| {},
    );
}

fn encode_binary_op(encoder: &mut Encoder, op: hir::BinaryOp) {
    encoder.variant(
        match op {
            hir::BinaryOp::Add => b"add",
            hir::BinaryOp::Sub => b"subtract",
            hir::BinaryOp::Mul => b"multiply",
            hir::BinaryOp::Div => b"divide",
            hir::BinaryOp::Rem => b"remainder",
            hir::BinaryOp::BitAnd => b"bit-and",
            hir::BinaryOp::BitOr => b"bit-or",
            hir::BinaryOp::BitXor => b"bit-xor",
            hir::BinaryOp::Shl => b"shift-left",
            hir::BinaryOp::Shr => b"shift-right",
            hir::BinaryOp::AndNot => b"and-not",
            hir::BinaryOp::Equal => b"equal",
            hir::BinaryOp::NotEqual => b"not-equal",
            hir::BinaryOp::Less => b"less",
            hir::BinaryOp::LessEqual => b"less-equal",
            hir::BinaryOp::Greater => b"greater",
            hir::BinaryOp::GreaterEqual => b"greater-equal",
            hir::BinaryOp::LogicalAnd => b"logical-and",
            hir::BinaryOp::LogicalOr => b"logical-or",
        },
        |_| {},
    );
}

fn encode_unary_op(encoder: &mut Encoder, op: hir::UnaryOp) {
    encoder.variant(
        match op {
            hir::UnaryOp::Positive => b"positive",
            hir::UnaryOp::Negative => b"negative",
            hir::UnaryOp::Not => b"not",
            hir::UnaryOp::BitNot => b"bit-not",
        },
        |_| {},
    );
}

fn encode_value_category(encoder: &mut Encoder, category: hir::ValueCategory) {
    encoder.variant(
        match category {
            hir::ValueCategory::Value => b"value",
            hir::ValueCategory::Place => b"place",
            hir::ValueCategory::Constant => b"constant",
        },
        |_| {},
    );
}
