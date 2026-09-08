//! Shared semantic dependency helpers.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::{Db, FunctionProjection, PackageInput, SourceInput, file_projection};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure};
use crate::compiler::ids::{
    DefId, DefinitionKey, DefinitionKind, FileId, PackageId, ReceiverIdentity,
};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ConstantSyntax, ConstantValueSyntax, DeclSyntax, ExprSyntax, ExprSyntaxKind,
    FieldListSyntax, FunctionBodySyntax, FunctionHeaderSyntax, StmtSyntax, StmtSyntaxKind,
    VariableSyntax, VariableValueSyntax,
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

pub(super) fn function_definition_key(
    package: PackageId,
    header: &FunctionHeaderSyntax,
) -> (DefinitionKey, Option<Arc<str>>, bool) {
    if let Some((receiver, pointer)) = crate::compiler::syntax::method_receiver(header) {
        (
            DefinitionKey::method(
                ReceiverIdentity::named(package, receiver),
                &*header.name.name,
            ),
            Some(Arc::from(receiver)),
            pointer,
        )
    } else {
        (
            DefinitionKey::package_named(package, DefinitionKind::Function, &*header.name.name),
            None,
            false,
        )
    }
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
    function_source(db, input, function).map(|source| source.file(db))
}

pub(super) fn function_source(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> Result<SourceInput, Arc<StageFailure>> {
    let definition = function.id(db);
    input
        .sources(db)
        .iter()
        .copied()
        .find(|source| {
            file_projection(db, *source)
                .functions(db)
                .into_iter()
                .any(|candidate| candidate.id(db) == definition)
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
    if let Some(explicit_type) = &syntax.explicit_type {
        collect_all_expression_names(explicit_type, names);
    }
    if let ConstantValueSyntax::Expression(expression) = &syntax.value {
        collect_all_expression_names(expression, names);
    }
}

pub(super) fn collect_variable_references(syntax: &VariableSyntax, names: &mut BTreeSet<String>) {
    if let Some(explicit_type) = &syntax.explicit_type {
        collect_all_expression_names(explicit_type, names);
    }
    if let VariableValueSyntax::Expression(expression) = &syntax.value {
        collect_all_expression_names(expression, names);
    }
}

/// Names a package variable initializer reads as values.
///
/// This is narrower than [`collect_variable_references`], which also feeds
/// constant resolution and so must keep every name a literal mentions. A
/// composite literal's element key is a field name for a struct literal and a
/// constant index for an array literal; only a map literal's key is itself a
/// value expression. Counting every key as a value makes a literal that names
/// one of its own fields look like it depends on the variable it initializes,
/// which is exactly the shape of `var Removed = []RemovedInfo{{Removed: 24}}`.
pub(super) fn collect_variable_value_names(syntax: &VariableSyntax, names: &mut BTreeSet<String>) {
    if let VariableValueSyntax::Expression(expression) = &syntax.value {
        collect_value_names(expression, false, names);
    }
}

fn collect_value_names(
    expression: &ExprSyntax,
    keys_are_values: bool,
    names: &mut BTreeSet<String>,
) {
    let ExprSyntaxKind::CompositeLiteral { ty, elements } = &expression.kind else {
        // Any other expression reads every name it mentions.
        collect_all_expression_names(expression, names);
        return;
    };
    if let Some(ty) = ty {
        collect_all_expression_names(ty, names);
    }
    // An elided nested literal inherits its parent's element type, so a map's
    // values are the only place a nested key stays a value expression.
    let nested_keys_are_values = match ty {
        Some(ty) => matches!(ty.kind, ExprSyntaxKind::MapType { .. }),
        None => keys_are_values,
    };
    for element in &**elements {
        if let ExprSyntaxKind::KeyValue { key, value } = &element.kind {
            if nested_keys_are_values {
                collect_value_names(key, nested_keys_are_values, names);
            }
            collect_value_names(value, nested_keys_are_values, names);
        } else {
            collect_value_names(element, nested_keys_are_values, names);
        }
    }
}

pub(super) fn collect_signature_type_references(
    header: &FunctionHeaderSyntax,
    names: &mut BTreeSet<String>,
) {
    if let Some(receiver) = &header.receiver {
        collect_field_type_names(receiver, names);
    }
    collect_field_type_names(&header.params, names);
    if let Some(results) = &header.results {
        collect_field_type_names(results, names);
    }
}

pub(super) struct PackageReferences {
    pub(super) unqualified: BTreeSet<Arc<str>>,
    pub(super) ordinary_unqualified: BTreeSet<Arc<str>>,
    pub(super) qualified: BTreeSet<(Arc<str>, Arc<str>)>,
    pub(super) method_names: BTreeSet<Arc<str>>,
    pub(super) range_functions: BTreeSet<Arc<str>>,
    pub(super) generic_calls: Vec<GenericCallReference>,
    pub(super) generic_instantiations: Vec<GenericInstantiationReference>,
}

pub(super) struct GenericCallReference {
    pub(super) callee: Arc<str>,
    pub(super) explicit_type_argument_names: Vec<Option<Arc<str>>>,
    pub(super) argument_type_names: Vec<Option<Arc<str>>>,
    pub(super) spread: bool,
}

pub(super) struct GenericInstantiationReference {
    pub(super) base: Arc<str>,
    pub(super) argument_type_names: Vec<Option<Arc<str>>>,
}

type GenericCalleeReference = (Arc<str>, Vec<Option<Arc<str>>>);

/// Collect package-level names referenced by a body after lexical shadowing.
pub(super) fn package_references_in_body(
    header: &FunctionHeaderSyntax,
    body: &FunctionBodySyntax,
) -> PackageReferences {
    let mut collector = PackageReferenceCollector {
        scopes: vec![BTreeMap::new()],
        unqualified: BTreeSet::new(),
        ordinary_unqualified: BTreeSet::new(),
        qualified: BTreeSet::new(),
        method_names: BTreeSet::new(),
        range_functions: BTreeSet::new(),
        generic_calls: Vec::new(),
        generic_instantiations: Vec::new(),
    };
    if let Some(receiver) = &header.receiver {
        collector.field_type_expressions(receiver);
        collector.bind_fields(receiver);
    }
    collector.field_type_expressions(&header.params);
    collector.bind_fields(&header.params);
    if let Some(results) = &header.results {
        collector.field_type_expressions(results);
        collector.bind_fields(results);
    }
    if let Some(body) = &body.block {
        collector.block(body, false);
    }
    PackageReferences {
        unqualified: collector.unqualified,
        ordinary_unqualified: collector.ordinary_unqualified,
        qualified: collector.qualified,
        method_names: collector.method_names,
        range_functions: collector.range_functions,
        generic_calls: collector.generic_calls,
        generic_instantiations: collector.generic_instantiations,
    }
}

/// Non-exported iterator declarations whose only value use in this package is
/// as a statically specialized range expression.
pub(super) fn specialized_range_iterator_ids(db: &dyn Db, input: PackageInput) -> BTreeSet<DefId> {
    let mut declarations = BTreeMap::<Arc<str>, (DefId, bool)>::new();
    let mut ranged = BTreeSet::<Arc<str>>::new();
    let mut ordinary = BTreeSet::<Arc<str>>::new();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        for function in file_projection(db, source).functions(db) {
            let header = function.signature(db);
            let structure = header.structure();
            if function.receiver_type(db).is_none() {
                declarations.insert(
                    function.name(db),
                    (
                        function.id(db),
                        crate::compiler::syntax::function_is_range_iterator(structure),
                    ),
                );
            }
            let references = package_references_in_body(structure, function.body(db).structure());
            ranged.extend(references.range_functions);
            ordinary.extend(references.ordinary_unqualified);
        }
    }
    ranged
        .difference(&ordinary)
        .filter_map(|name| {
            let (definition, iterator_shape) = declarations.get(name)?;
            (*iterator_shape && !is_exported_name(name)).then_some(*definition)
        })
        .collect()
}

fn is_exported_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

struct PackageReferenceCollector {
    scopes: Vec<BTreeMap<Arc<str>, Option<Arc<str>>>>,
    unqualified: BTreeSet<Arc<str>>,
    ordinary_unqualified: BTreeSet<Arc<str>>,
    qualified: BTreeSet<(Arc<str>, Arc<str>)>,
    method_names: BTreeSet<Arc<str>>,
    range_functions: BTreeSet<Arc<str>>,
    generic_calls: Vec<GenericCallReference>,
    generic_instantiations: Vec<GenericInstantiationReference>,
}

impl PackageReferenceCollector {
    fn bind_fields(&mut self, fields: &crate::compiler::syntax::FieldListSyntax) {
        for field in &*fields.fields {
            let ty = field.ty.as_ref().and_then(|ty| self.type_name(ty));
            if let Some(names) = &field.names {
                for name in &**names {
                    self.bind_typed(Arc::clone(&name.name), ty.clone());
                }
            }
        }
    }

    fn bind(&mut self, name: Arc<str>) {
        self.bind_typed(name, None);
    }

    fn bind_typed(&mut self, name: Arc<str>, ty: Option<Arc<str>>) {
        if name.as_ref() != "_" {
            debug_assert!(
                !self.scopes.is_empty(),
                "reference collector always owns a function scope"
            );
            if let Some(scope) = self.scopes.last_mut() {
                scope.entry(name).or_insert(ty);
            }
        }
    }

    fn is_bound(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.contains_key(name))
    }

    fn bound_type_name(&self, name: &str) -> Option<Arc<str>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .flatten()
    }

    fn block(&mut self, block: &BlockSyntax, introduce_scope: bool) {
        if introduce_scope {
            self.scopes.push(BTreeMap::new());
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
                let inferred_types = (*token == Token::DEFINE).then(|| {
                    right
                        .iter()
                        .map(|expression| self.receiver_type_name(expression))
                        .collect::<Vec<_>>()
                });
                for expression in &**right {
                    self.expression(expression);
                }
                if *token == Token::DEFINE {
                    for (index, expression) in left.iter().enumerate() {
                        if let ExprSyntaxKind::Ident(ident) = &expression.kind {
                            self.bind_typed(
                                Arc::clone(&ident.name),
                                inferred_types
                                    .as_ref()
                                    .and_then(|types| types.get(index))
                                    .cloned()
                                    .flatten(),
                            );
                        }
                    }
                } else {
                    for expression in &**left {
                        self.expression(expression);
                    }
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
                self.scopes.push(BTreeMap::new());
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
                self.scopes.push(BTreeMap::new());
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
                self.scopes.push(BTreeMap::new());
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
                if let ExprSyntaxKind::Ident(identifier) = &expression.kind
                    && !self.is_bound(&identifier.name)
                {
                    self.unqualified.insert(Arc::clone(&identifier.name));
                    self.range_functions.insert(Arc::clone(&identifier.name));
                } else {
                    self.expression(expression);
                }
                self.scopes.push(BTreeMap::new());
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
                self.scopes.push(BTreeMap::new());
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
            StmtSyntaxKind::TypeSwitch {
                init,
                binding,
                expression,
                cases,
            } => {
                self.scopes.push(BTreeMap::new());
                if let Some(init) = init {
                    self.statement(init);
                }
                self.expression(expression);
                for case in &**cases {
                    for expression in &*case.expressions {
                        self.expression(expression);
                    }
                    self.scopes.push(BTreeMap::new());
                    if let Some(binding) = binding {
                        self.bind(Arc::clone(&binding.name));
                    }
                    self.block(&case.body, false);
                    self.scopes.pop();
                }
                self.scopes.pop();
            }
            StmtSyntaxKind::Select { cases } => {
                for case in &**cases {
                    self.scopes.push(BTreeMap::new());
                    if let Some(communication) = &case.communication {
                        self.statement(communication);
                    }
                    self.block(&case.body, false);
                    self.scopes.pop();
                }
            }
            StmtSyntaxKind::Labeled { statement, .. } => self.statement(statement),
        }
    }

    fn declaration(&mut self, declaration: &DeclSyntax) {
        for spec in &*declaration.type_specs {
            self.bind(Arc::clone(&spec.name.name));
            self.expression(&spec.target);
        }
        for spec in &*declaration.specs {
            // Go evaluates a ValueSpec's RHS before introducing its names.
            if let Some(explicit_type) = &spec.explicit_type {
                self.expression(explicit_type);
            }
            if let Some(values) = &spec.values {
                for value in &**values {
                    self.expression(value);
                }
            }
            let explicit_type = spec
                .explicit_type
                .as_ref()
                .and_then(|ty| self.type_name(ty));
            for (index, name) in spec.names.iter().enumerate() {
                let inferred_type = explicit_type.clone().or_else(|| {
                    spec.values
                        .as_ref()
                        .and_then(|values| values.get(index))
                        .and_then(|value| self.receiver_type_name(value))
                });
                self.bind_typed(Arc::clone(&name.name), inferred_type);
            }
        }
    }

    fn expression(&mut self, expression: &ExprSyntax) {
        match &expression.kind {
            ExprSyntaxKind::Ident(ident) => {
                if !self.is_bound(&ident.name) {
                    self.unqualified.insert(Arc::clone(&ident.name));
                    self.ordinary_unqualified.insert(Arc::clone(&ident.name));
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
                callee,
                arguments,
                spread,
            } => {
                if let Some((callee, explicit_type_argument_names)) =
                    self.generic_callee_reference(callee)
                {
                    let argument_type_names = arguments
                        .iter()
                        .map(|argument| self.receiver_type_name(argument))
                        .collect();
                    self.generic_calls.push(GenericCallReference {
                        callee,
                        explicit_type_argument_names,
                        argument_type_names,
                        spread: *spread,
                    });
                }
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
                self.scopes.push(BTreeMap::new());
                self.bind_fields(params);
                if let Some(results) = results {
                    self.bind_fields(results);
                }
                self.block(body, false);
                self.scopes.pop();
            }
            ExprSyntaxKind::FunctionType {
                params, results, ..
            } => {
                self.field_type_expressions(params);
                if let Some(results) = results {
                    self.field_type_expressions(results);
                }
            }
            ExprSyntaxKind::Selector { base, member } => {
                if let ExprSyntaxKind::Ident(ident) = &base.kind
                    && !self.is_bound(&ident.name)
                {
                    self.qualified
                        .insert((Arc::clone(&ident.name), Arc::clone(&member.name)));
                } else {
                    self.method_names.insert(Arc::clone(&member.name));
                    self.expression(base);
                }
            }
            ExprSyntaxKind::TypeAssert { value, asserted } => {
                self.expression(value);
                if let Some(asserted) = asserted {
                    self.expression(asserted);
                }
            }
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
            ExprSyntaxKind::StructType { fields }
            | ExprSyntaxKind::InterfaceType { methods: fields } => {
                self.field_type_expressions(fields);
            }
            ExprSyntaxKind::CompositeLiteral { ty, elements } => {
                if let Some(ty) = ty {
                    self.expression(ty);
                }
                for element in &**elements {
                    self.expression(element);
                }
            }
            ExprSyntaxKind::Index { base, index } => {
                if let ExprSyntaxKind::Ident(base) = &base.kind
                    && !self.is_bound(&base.name)
                {
                    self.generic_instantiations
                        .push(GenericInstantiationReference {
                            base: Arc::clone(&base.name),
                            argument_type_names: vec![self.type_name(index)],
                        });
                }
                self.expression(base);
                self.expression(index);
            }
            ExprSyntaxKind::IndexList { base, indices } => {
                if let ExprSyntaxKind::Ident(base) = &base.kind
                    && !self.is_bound(&base.name)
                {
                    self.generic_instantiations
                        .push(GenericInstantiationReference {
                            base: Arc::clone(&base.name),
                            argument_type_names: indices
                                .iter()
                                .map(|argument| self.type_name(argument))
                                .collect(),
                        });
                }
                self.expression(base);
                for index in &**indices {
                    self.expression(index);
                }
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

    fn field_type_expressions(&mut self, fields: &FieldListSyntax) {
        for field in &*fields.fields {
            if let Some(ty) = &field.ty {
                self.expression(ty);
            }
        }
    }

    fn generic_callee_name(&self, callee: &ExprSyntax) -> Option<Arc<str>> {
        self.generic_callee_reference(callee).map(|(name, _)| name)
    }

    fn generic_callee_reference(&self, callee: &ExprSyntax) -> Option<GenericCalleeReference> {
        match &callee.kind {
            ExprSyntaxKind::Ident(ident) if !self.is_bound(&ident.name) => {
                Some((Arc::clone(&ident.name), Vec::new()))
            }
            ExprSyntaxKind::Index { base, index } => {
                let ExprSyntaxKind::Ident(ident) = &base.kind else {
                    return None;
                };
                (!self.is_bound(&ident.name))
                    .then(|| (Arc::clone(&ident.name), vec![self.type_name(index)]))
            }
            ExprSyntaxKind::IndexList { base, indices } => {
                let ExprSyntaxKind::Ident(ident) = &base.kind else {
                    return None;
                };
                (!self.is_bound(&ident.name)).then(|| {
                    (
                        Arc::clone(&ident.name),
                        indices
                            .iter()
                            .map(|argument| self.type_name(argument))
                            .collect(),
                    )
                })
            }
            _ => None,
        }
    }

    fn receiver_type_name(&self, expression: &ExprSyntax) -> Option<Arc<str>> {
        match &expression.kind {
            ExprSyntaxKind::Ident(ident) => {
                if self.is_bound(&ident.name) {
                    self.bound_type_name(&ident.name)
                } else {
                    Some(Arc::clone(&ident.name))
                }
            }
            ExprSyntaxKind::Paren(inner)
            | ExprSyntaxKind::Unary {
                expression: inner, ..
            } => self.receiver_type_name(inner),
            ExprSyntaxKind::CompositeLiteral { ty: Some(ty), .. } => self.type_name(ty),
            ExprSyntaxKind::Call { callee, .. } => self.generic_callee_name(callee),
            _ => None,
        }
    }

    fn type_name(&self, expression: &ExprSyntax) -> Option<Arc<str>> {
        match &expression.kind {
            ExprSyntaxKind::Ident(ident) if !self.is_bound(&ident.name) => {
                Some(Arc::clone(&ident.name))
            }
            ExprSyntaxKind::Paren(inner)
            | ExprSyntaxKind::Unary {
                expression: inner, ..
            } => self.type_name(inner),
            ExprSyntaxKind::Index { base, .. } | ExprSyntaxKind::IndexList { base, .. } => {
                self.type_name(base)
            }
            _ => None,
        }
    }
}

pub(super) fn collect_all_expression_names(expression: &ExprSyntax, names: &mut BTreeSet<String>) {
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
        }
        | ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            collect_field_type_names(params, names);
            if let Some(results) = results {
                collect_field_type_names(results, names);
            }
        }
        ExprSyntaxKind::Selector { base, .. } => collect_all_expression_names(base, names),
        ExprSyntaxKind::TypeAssert { value, asserted } => {
            collect_all_expression_names(value, names);
            if let Some(asserted) = asserted {
                collect_all_expression_names(asserted, names);
            }
        }
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
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            collect_field_type_names(fields, names);
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
        ExprSyntaxKind::IndexList { base, indices } => {
            collect_all_expression_names(base, names);
            for index in &**indices {
                collect_all_expression_names(index, names);
            }
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

fn collect_field_type_names(fields: &FieldListSyntax, names: &mut BTreeSet<String>) {
    for field in &*fields.fields {
        if let Some(ty) = &field.ty {
            collect_all_expression_names(ty, names);
        }
    }
}
