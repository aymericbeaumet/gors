//! Shared semantic dependency helpers.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{Db, FunctionProjection, PackageInput, file_projection};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure};
use crate::compiler::ids::{DefId, FileId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ConstantSyntax, ConstantValueSyntax, DeclSyntax, ExprSyntax, ExprSyntaxKind,
    FunctionBodySyntax, FunctionHeaderSyntax, StmtSyntax, StmtSyntaxKind,
};
use crate::token::Token;

pub(super) fn semantic_build_dependency(
    db: &dyn Db,
    definition: DefId,
) -> Result<(), Arc<StageFailure>> {
    let build = db.query_build_input().ok_or_else(|| {
        Arc::new(StageFailure::one_for_definition(
            CompilerStage::Semantic,
            definition,
            Diagnostic::backend(
                "compiler build config is missing from the semantic query database",
            ),
        ))
    })?;
    let _go_version = build.go_version(db);
    Ok(())
}

pub(super) fn check_semantic_barrier(
    db: &dyn Db,
    definition: DefId,
    barrier: Option<Arc<str>>,
) -> Result<(), Arc<StageFailure>> {
    db.unwind_if_revision_cancelled();
    barrier.map_or(Ok(()), |message| {
        Err(Arc::new(StageFailure::one_for_definition(
            CompilerStage::Semantic,
            definition,
            Diagnostic::unsupported(message.to_string(), SourceRef::definition(definition)),
        )))
    })
}

pub(super) fn semantic_failure(definition: DefId, diagnostic: Diagnostic) -> Arc<StageFailure> {
    Arc::new(StageFailure::one_for_definition(
        CompilerStage::Semantic,
        definition,
        diagnostic,
    ))
}

pub(super) fn function_file(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> Result<FileId, Arc<StageFailure>> {
    let definition = function.id(db);
    input
        .sources(db)
        .iter()
        .copied()
        .find_map(|source| {
            file_projection(db, source)
                .functions(db)
                .into_iter()
                .any(|candidate| candidate.id(db) == definition)
                .then(|| source.file(db))
        })
        .ok_or_else(|| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::Semantic,
                definition,
                Diagnostic::backend("function projection is absent from its owning package input"),
            ))
        })
}

pub(super) fn collect_constant_references(syntax: &ConstantSyntax, names: &mut BTreeSet<String>) {
    if let ConstantValueSyntax::Expression(expression) = &syntax.value {
        collect_all_expression_names(expression, names);
    }
}

/// Collect package-level names referenced by a body after lexical shadowing.
pub(super) fn referenced_names_in_body(
    header: &FunctionHeaderSyntax,
    body: &FunctionBodySyntax,
) -> BTreeSet<Arc<str>> {
    let mut collector = PackageReferenceCollector {
        scopes: vec![BTreeSet::new()],
        referenced: BTreeSet::new(),
    };
    collector.bind_fields(&header.params);
    if let Some(results) = &header.results {
        collector.bind_fields(results);
    }
    if let Some(body) = &body.block {
        collector.block(body, false);
    }
    collector.referenced
}

struct PackageReferenceCollector {
    scopes: Vec<BTreeSet<Arc<str>>>,
    referenced: BTreeSet<Arc<str>>,
}

impl PackageReferenceCollector {
    fn bind_fields(&mut self, fields: &crate::compiler::syntax::FieldListSyntax) {
        for field in &*fields.fields {
            if let Some(names) = &field.names {
                for name in &**names {
                    self.bind(Arc::clone(&name.name));
                }
            }
        }
    }

    fn bind(&mut self, name: Arc<str>) {
        if name.as_ref() != "_" {
            debug_assert!(
                !self.scopes.is_empty(),
                "reference collector always owns a function scope"
            );
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert(name);
            }
        }
    }

    fn is_bound(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn block(&mut self, block: &BlockSyntax, introduce_scope: bool) {
        if introduce_scope {
            self.scopes.push(BTreeSet::new());
        }
        for statement in &*block.statements {
            self.statement(statement);
        }
        if introduce_scope {
            self.scopes.pop();
        }
    }

    fn statement(&mut self, statement: &StmtSyntax) {
        match &statement.kind {
            StmtSyntaxKind::Empty
            | StmtSyntaxKind::Branch { .. }
            | StmtSyntaxKind::Unsupported(_) => {}
            StmtSyntaxKind::Block(block) => self.block(block, true),
            StmtSyntaxKind::Expr(expression) => self.expression(expression),
            StmtSyntaxKind::Decl(declaration) => self.declaration(declaration),
            StmtSyntaxKind::Assign { left, token, right } => {
                for expression in &**right {
                    self.expression(expression);
                }
                if *token == Token::DEFINE {
                    for expression in &**left {
                        if let ExprSyntaxKind::Ident(ident) = &expression.kind {
                            self.bind(Arc::clone(&ident.name));
                        }
                    }
                }
            }
            // Assignment and increment targets must already be locals. They
            // are not package references even when the statement is invalid.
            StmtSyntaxKind::IncDec { .. } => {}
            StmtSyntaxKind::Send { channel, value } => {
                self.expression(channel);
                self.expression(value);
            }
            StmtSyntaxKind::Defer {
                params,
                results,
                body,
                arguments,
                ..
            } => {
                for argument in &**arguments {
                    self.expression(argument);
                }
                for field in &*params.fields {
                    if let Some(ty) = &field.ty {
                        self.expression(ty);
                    }
                }
                if let Some(results) = results {
                    for field in &*results.fields {
                        if let Some(ty) = &field.ty {
                            self.expression(ty);
                        }
                    }
                }
                self.scopes.push(BTreeSet::new());
                self.bind_fields(params);
                if let Some(results) = results {
                    self.bind_fields(results);
                }
                self.block(body, false);
                self.scopes.pop();
            }
            StmtSyntaxKind::Return(expressions) => {
                for expression in &**expressions {
                    self.expression(expression);
                }
            }
            StmtSyntaxKind::If {
                init,
                condition,
                then_block,
                else_branch,
            } => {
                self.scopes.push(BTreeSet::new());
                if let Some(init) = init {
                    self.statement(init);
                }
                self.expression(condition);
                self.block(then_block, true);
                if let Some(branch) = else_branch {
                    self.statement(branch);
                }
                self.scopes.pop();
            }
            StmtSyntaxKind::For {
                label: _,
                init,
                condition,
                post,
                body,
            } => {
                self.scopes.push(BTreeSet::new());
                if let Some(init) = init {
                    self.statement(init);
                }
                if let Some(condition) = condition {
                    self.expression(condition);
                }
                self.block(body, true);
                if let Some(post) = post {
                    self.statement(post);
                }
                self.scopes.pop();
            }
            StmtSyntaxKind::Range {
                key,
                value,
                token,
                expression,
                body,
                ..
            } => {
                self.expression(expression);
                self.scopes.push(BTreeSet::new());
                if *token == Some(Token::DEFINE) {
                    for target in [key, value].into_iter().flatten() {
                        if let ExprSyntaxKind::Ident(ident) = &target.kind {
                            self.bind(Arc::clone(&ident.name));
                        }
                    }
                }
                self.block(body, true);
                self.scopes.pop();
            }
            StmtSyntaxKind::Switch { init, tag, cases } => {
                self.scopes.push(BTreeSet::new());
                if let Some(init) = init {
                    self.statement(init);
                }
                if let Some(tag) = tag {
                    self.expression(tag);
                }
                for case in &**cases {
                    for expression in &*case.expressions {
                        self.expression(expression);
                    }
                    self.block(&case.body, true);
                }
                self.scopes.pop();
            }
            StmtSyntaxKind::Labeled { statement, .. } => self.statement(statement),
        }
    }

    fn declaration(&mut self, declaration: &DeclSyntax) {
        for spec in &*declaration.specs {
            // Go evaluates a ValueSpec's RHS before introducing its names.
            if let Some(values) = &spec.values {
                for value in &**values {
                    self.expression(value);
                }
            }
            for name in &*spec.names {
                self.bind(Arc::clone(&name.name));
            }
        }
    }

    fn expression(&mut self, expression: &ExprSyntax) {
        match &expression.kind {
            ExprSyntaxKind::Ident(ident) => {
                if !self.is_bound(&ident.name) {
                    self.referenced.insert(Arc::clone(&ident.name));
                }
            }
            ExprSyntaxKind::Paren(expression) | ExprSyntaxKind::Unary { expression, .. } => {
                self.expression(expression);
            }
            ExprSyntaxKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            ExprSyntaxKind::Call {
                callee, arguments, ..
            } => {
                self.expression(callee);
                for argument in &**arguments {
                    self.expression(argument);
                }
            }
            ExprSyntaxKind::FunctionLiteral {
                params,
                results,
                body,
                ..
            } => {
                for field in &*params.fields {
                    if let Some(ty) = &field.ty {
                        self.expression(ty);
                    }
                }
                if let Some(results) = results {
                    for field in &*results.fields {
                        if let Some(ty) = &field.ty {
                            self.expression(ty);
                        }
                    }
                }
                self.scopes.push(BTreeSet::new());
                self.bind_fields(params);
                if let Some(results) = results {
                    self.bind_fields(results);
                }
                self.block(body, false);
                self.scopes.pop();
            }
            ExprSyntaxKind::Selector { base, .. } => self.expression(base),
            ExprSyntaxKind::ArrayType { length, element } => {
                if let Some(length) = length {
                    self.expression(length);
                }
                self.expression(element);
            }
            ExprSyntaxKind::MapType { key, value } | ExprSyntaxKind::KeyValue { key, value } => {
                self.expression(key);
                self.expression(value);
            }
            ExprSyntaxKind::ChannelType { element, .. } => self.expression(element),
            ExprSyntaxKind::CompositeLiteral { ty, elements } => {
                if let Some(ty) = ty {
                    self.expression(ty);
                }
                for element in &**elements {
                    self.expression(element);
                }
            }
            ExprSyntaxKind::Index { base, index } => {
                self.expression(base);
                self.expression(index);
            }
            ExprSyntaxKind::Slice {
                base,
                low,
                high,
                max,
            } => {
                self.expression(base);
                for bound in [low, high, max].into_iter().flatten() {
                    self.expression(bound);
                }
            }
            ExprSyntaxKind::Literal { .. } | ExprSyntaxKind::Unsupported(_) => {}
        }
    }
}

fn collect_all_expression_names(expression: &ExprSyntax, names: &mut BTreeSet<String>) {
    match &expression.kind {
        ExprSyntaxKind::Ident(ident) => {
            names.insert(ident.name.to_string());
        }
        ExprSyntaxKind::Paren(expression) | ExprSyntaxKind::Unary { expression, .. } => {
            collect_all_expression_names(expression, names);
        }
        ExprSyntaxKind::Binary { left, right, .. } => {
            collect_all_expression_names(left, names);
            collect_all_expression_names(right, names);
        }
        ExprSyntaxKind::Call {
            callee, arguments, ..
        } => {
            collect_all_expression_names(callee, names);
            for argument in &**arguments {
                collect_all_expression_names(argument, names);
            }
        }
        ExprSyntaxKind::FunctionLiteral {
            params, results, ..
        } => {
            for field in &*params.fields {
                if let Some(ty) = &field.ty {
                    collect_all_expression_names(ty, names);
                }
            }
            if let Some(results) = results {
                for field in &*results.fields {
                    if let Some(ty) = &field.ty {
                        collect_all_expression_names(ty, names);
                    }
                }
            }
        }
        ExprSyntaxKind::Selector { base, .. } => collect_all_expression_names(base, names),
        ExprSyntaxKind::ArrayType { length, element } => {
            if let Some(length) = length {
                collect_all_expression_names(length, names);
            }
            collect_all_expression_names(element, names);
        }
        ExprSyntaxKind::MapType { key, value } | ExprSyntaxKind::KeyValue { key, value } => {
            collect_all_expression_names(key, names);
            collect_all_expression_names(value, names);
        }
        ExprSyntaxKind::ChannelType { element, .. } => {
            collect_all_expression_names(element, names);
        }
        ExprSyntaxKind::CompositeLiteral { ty, elements } => {
            if let Some(ty) = ty {
                collect_all_expression_names(ty, names);
            }
            for element in &**elements {
                collect_all_expression_names(element, names);
            }
        }
        ExprSyntaxKind::Index { base, index } => {
            collect_all_expression_names(base, names);
            collect_all_expression_names(index, names);
        }
        ExprSyntaxKind::Slice {
            base,
            low,
            high,
            max,
        } => {
            collect_all_expression_names(base, names);
            for bound in [low, high, max].into_iter().flatten() {
                collect_all_expression_names(bound, names);
            }
        }
        ExprSyntaxKind::Literal { .. } | ExprSyntaxKind::Unsupported(_) => {}
    }
}
