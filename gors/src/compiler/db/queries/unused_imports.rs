//! Spec-mandated per-file rejection of imported-and-not-used packages.
//!
//! Go requires every non-blank, non-dot import binding to be referenced at
//! least once inside its own file. The check consumes the file's resolved
//! import bindings and an over-approximating reference walk over that file's
//! owned declaration syntax, so it never rejects a program whose binding is
//! referenced. Files containing unsupported syntax skip the check because
//! their reference evidence is incomplete.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{Db, FileFacts, SourceInput};
use crate::compiler::db::ResolvedImportBinding;
use crate::compiler::db::model::PackageIssue;
use crate::compiler::syntax::{
    BlockSyntax, ConstantValueSyntax, DeclSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax,
    FunctionBodySyntax, FunctionHeaderSyntax, StmtSyntax, StmtSyntaxKind, VariableValueSyntax,
};

/// Per-file unused-import issues for one admitted source file.
pub(super) fn file_unused_import_issues(
    db: &dyn Db,
    source: SourceInput,
    facts: FileFacts<'_>,
) -> Vec<PackageIssue> {
    let resolved = source.resolved_imports(db).value(db);
    let ordinary = resolved
        .imports()
        .iter()
        .filter_map(|import| match import.binding() {
            ResolvedImportBinding::Default { local_name, .. } => {
                Some((Arc::clone(local_name), import, false))
            }
            ResolvedImportBinding::Named { local_name, .. } => {
                Some((Arc::clone(local_name), import, true))
            }
            // Blank imports are side-effect-only and exempt. Dot imports do
            // not introduce a package name, so this check does not own them.
            ResolvedImportBinding::Blank { .. } | ResolvedImportBinding::Dot { .. } => None,
        })
        .collect::<Vec<_>>();
    if ordinary.is_empty() {
        return Vec::new();
    }
    // Projection failures hide declaration syntax, so reference evidence for
    // this file is incomplete and a rejection could be unsound.
    if !facts.issues(db).is_empty() {
        return Vec::new();
    }
    let mut references = FileReferences::default();
    for function in facts.functions(db) {
        references.function(
            function.signature(db).structure(),
            function.body(db).structure(),
        );
    }
    for constant in facts.constants(db) {
        let syntax = constant.syntax(db);
        references.optional_expression(syntax.explicit_type.as_ref());
        if let ConstantValueSyntax::Expression(expression) = &syntax.value {
            references.expression(expression);
        }
    }
    for variable in facts.variables(db) {
        let syntax = variable.syntax(db);
        references.optional_expression(syntax.explicit_type.as_ref());
        if let VariableValueSyntax::Expression(expression) = &syntax.value {
            references.expression(expression);
        }
    }
    for alias in facts.type_aliases(db) {
        let syntax = alias.syntax(db);
        references.optional_field_types(syntax.type_parameters.as_ref());
        references.expression(&syntax.target);
    }
    for definition in facts.type_definitions(db) {
        let syntax = definition.syntax(db);
        references.optional_field_types(syntax.type_parameters.as_ref());
        references.expression(&syntax.underlying);
    }
    if references.incomplete {
        return Vec::new();
    }
    let imports = facts.imports(db);
    let occurrences = imports
        .direct()
        .iter()
        .map(|occurrence| (occurrence.source(), occurrence))
        .collect::<BTreeMap<_, _>>();
    let file = facts.file(db);
    ordinary
        .into_iter()
        .filter(|(local_name, _, _)| !references.names.contains(local_name.as_ref()))
        .map(|(local_name, import, named)| {
            let occurrence = occurrences.get(&import.source());
            PackageIssue::UnusedImport {
                file,
                local_name,
                path: Arc::from(import.canonical_path().as_str()),
                named,
                line: occurrence.map_or(0, |occurrence| occurrence.line()),
                column: occurrence.map_or(0, |occurrence| occurrence.column()),
                virtual_file: occurrence
                    .and_then(|occurrence| occurrence.virtual_file().map(Arc::from)),
            }
        })
        .collect()
}

/// Every identifier that could reference a file-scope import binding.
///
/// The walk deliberately over-approximates: it performs no lexical scope
/// tracking, so a shadowed name still counts as a reference and can only make
/// the check accept a program Go rejects, never the reverse. Selector members
/// are not lexical references and are excluded.
#[derive(Default)]
struct FileReferences {
    names: BTreeSet<Arc<str>>,
    incomplete: bool,
}

impl FileReferences {
    fn function(&mut self, header: &FunctionHeaderSyntax, body: &FunctionBodySyntax) {
        if let Some(receiver) = &header.receiver {
            self.field_types(receiver);
        }
        self.optional_field_types(header.type_parameters.as_ref());
        self.field_types(&header.params);
        self.optional_field_types(header.results.as_ref());
        if let Some(block) = &body.block {
            self.block(block);
        }
    }

    fn block(&mut self, block: &BlockSyntax) {
        for statement in &*block.statements {
            self.statement(statement);
        }
    }

    fn statement(&mut self, statement: &StmtSyntax) {
        match &statement.kind {
            StmtSyntaxKind::Empty | StmtSyntaxKind::Branch { .. } => {}
            StmtSyntaxKind::Unsupported(_) => self.incomplete = true,
            StmtSyntaxKind::Block(block) => self.block(block),
            StmtSyntaxKind::Expr(expression) => self.expression(expression),
            StmtSyntaxKind::Decl(declaration) => self.declaration(declaration),
            StmtSyntaxKind::Assign { left, right, .. } => {
                for expression in left.iter().chain(&**right) {
                    self.expression(expression);
                }
            }
            StmtSyntaxKind::IncDec { expression, .. } => self.expression(expression),
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
            }
            | StmtSyntaxKind::Go {
                params,
                results,
                body,
                arguments,
                ..
            } => {
                self.field_types(params);
                self.optional_field_types(results.as_ref());
                self.block(body);
                for argument in &**arguments {
                    self.expression(argument);
                }
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
                self.optional_statement(init.as_deref());
                self.expression(condition);
                self.block(then_block);
                self.optional_statement(else_branch.as_deref());
            }
            StmtSyntaxKind::For {
                init,
                condition,
                post,
                body,
                ..
            } => {
                self.optional_statement(init.as_deref());
                self.optional_expression(condition.as_ref());
                self.optional_statement(post.as_deref());
                self.block(body);
            }
            StmtSyntaxKind::Range {
                key,
                value,
                expression,
                body,
                ..
            } => {
                self.optional_expression(key.as_ref());
                self.optional_expression(value.as_ref());
                self.expression(expression);
                self.block(body);
            }
            StmtSyntaxKind::Switch { init, tag, cases } => {
                self.optional_statement(init.as_deref());
                self.optional_expression(tag.as_ref());
                for case in &**cases {
                    for expression in &*case.expressions {
                        self.expression(expression);
                    }
                    self.block(&case.body);
                }
            }
            StmtSyntaxKind::TypeSwitch {
                init,
                expression,
                cases,
                ..
            } => {
                self.optional_statement(init.as_deref());
                self.expression(expression);
                for case in &**cases {
                    for expression in &*case.expressions {
                        self.expression(expression);
                    }
                    self.block(&case.body);
                }
            }
            StmtSyntaxKind::Select { cases } => {
                for case in &**cases {
                    self.optional_statement(case.communication.as_deref());
                    self.block(&case.body);
                }
            }
            StmtSyntaxKind::Labeled { statement, .. } => self.statement(statement),
        }
    }

    fn declaration(&mut self, declaration: &DeclSyntax) {
        for spec in &*declaration.type_specs {
            self.expression(&spec.target);
        }
        for spec in &*declaration.specs {
            self.optional_expression(spec.explicit_type.as_ref());
            for value in spec.values.iter().flat_map(|values| &**values) {
                self.expression(value);
            }
        }
    }

    fn expression(&mut self, expression: &ExprSyntax) {
        match &expression.kind {
            ExprSyntaxKind::Ident(identifier) => {
                self.names.insert(Arc::clone(&identifier.name));
            }
            ExprSyntaxKind::Literal { .. } => {}
            ExprSyntaxKind::Unsupported(_) => self.incomplete = true,
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
                self.field_types(params);
                self.optional_field_types(results.as_ref());
                self.block(body);
            }
            ExprSyntaxKind::FunctionType {
                params, results, ..
            } => {
                self.field_types(params);
                self.optional_field_types(results.as_ref());
            }
            // A selector member is resolved inside its base and is never a
            // file-scope reference; only the base can name an import.
            ExprSyntaxKind::Selector { base, .. } => self.expression(base),
            ExprSyntaxKind::TypeAssert { value, asserted } => {
                self.expression(value);
                self.optional_expression(asserted.as_deref());
            }
            ExprSyntaxKind::ArrayType { length, element } => {
                self.optional_expression(length.as_deref());
                self.expression(element);
            }
            ExprSyntaxKind::MapType { key, value } | ExprSyntaxKind::KeyValue { key, value } => {
                self.expression(key);
                self.expression(value);
            }
            ExprSyntaxKind::ChannelType { element, .. } => self.expression(element),
            ExprSyntaxKind::StructType { fields }
            | ExprSyntaxKind::InterfaceType { methods: fields } => self.field_types(fields),
            ExprSyntaxKind::CompositeLiteral { ty, elements } => {
                self.optional_expression(ty.as_deref());
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
        }
    }

    fn optional_statement(&mut self, statement: Option<&StmtSyntax>) {
        if let Some(statement) = statement {
            self.statement(statement);
        }
    }

    fn optional_expression(&mut self, expression: Option<&ExprSyntax>) {
        if let Some(expression) = expression {
            self.expression(expression);
        }
    }

    fn field_types(&mut self, fields: &FieldListSyntax) {
        for field in &*fields.fields {
            if let Some(ty) = &field.ty {
                self.expression(ty);
            }
        }
    }

    fn optional_field_types(&mut self, fields: Option<&FieldListSyntax>) {
        if let Some(fields) = fields {
            self.field_types(fields);
        }
    }
}
