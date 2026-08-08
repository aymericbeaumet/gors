//! Function-scoped label resolution and lexical legality checks for `goto`.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::compiler::syntax::{
    BlockSyntax, DeclSyntax, ExprSyntaxKind, StmtSyntax, StmtSyntaxKind, SyntaxSource,
};
use crate::token::Token;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GotoScopeError {
    pub(super) message: String,
    pub(super) source: SyntaxSource,
}

#[derive(Clone, Debug)]
struct Scope {
    id: usize,
    bindings: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
struct Position {
    scope_path: Vec<usize>,
    bindings: BTreeSet<usize>,
}

#[derive(Clone, Debug)]
struct GotoUse {
    label: String,
    source: SyntaxSource,
    position: Position,
}

struct Analyzer {
    next_scope: usize,
    next_binding: usize,
    scopes: Vec<Scope>,
    binding_names: BTreeMap<usize, String>,
    /// A repeated label is represented by `None` so the ordinary duplicate
    /// label diagnostic retains precedence over a derived goto-scope error.
    labels: BTreeMap<String, Option<Position>>,
    gotos: Vec<GotoUse>,
}

pub(super) fn validate(block: &BlockSyntax) -> Result<(), GotoScopeError> {
    let mut analyzer = Analyzer {
        next_scope: 1,
        next_binding: 0,
        scopes: vec![Scope {
            id: 0,
            bindings: BTreeMap::new(),
        }],
        binding_names: BTreeMap::new(),
        labels: BTreeMap::new(),
        gotos: Vec::new(),
    };
    analyzer.walk_block(block, false);
    analyzer.finish()
}

impl Analyzer {
    fn finish(self) -> Result<(), GotoScopeError> {
        for jump in &self.gotos {
            let Some(Some(target)) = self.labels.get(&jump.label) else {
                // Undefined and duplicate labels are diagnosed by ordinary
                // semantic lowering, which owns those established messages.
                continue;
            };
            if !target.scope_path.iter().eq(jump
                .position
                .scope_path
                .iter()
                .take(target.scope_path.len()))
            {
                return Err(GotoScopeError {
                    message: format!("goto {} jumps into block", jump.label),
                    source: jump.source,
                });
            }
            if let Some(binding) = target.bindings.difference(&jump.position.bindings).next() {
                let name = self
                    .binding_names
                    .get(binding)
                    .map(String::as_str)
                    .unwrap_or("variable");
                return Err(GotoScopeError {
                    message: format!("goto {} jumps over declaration of {name}", jump.label),
                    source: jump.source,
                });
            }
        }
        Ok(())
    }

    fn walk_block(&mut self, block: &BlockSyntax, introduce_scope: bool) {
        if introduce_scope {
            self.push_scope();
        }
        for statement in &*block.statements {
            self.walk_stmt(statement);
        }
        if introduce_scope {
            self.pop_scope();
        }
    }

    fn walk_stmt(&mut self, statement: &StmtSyntax) {
        match &statement.kind {
            StmtSyntaxKind::Empty
            | StmtSyntaxKind::Expr(_)
            | StmtSyntaxKind::IncDec { .. }
            | StmtSyntaxKind::Send { .. }
            | StmtSyntaxKind::Defer { .. }
            | StmtSyntaxKind::Go { .. }
            | StmtSyntaxKind::Return(_)
            | StmtSyntaxKind::Unsupported(_) => {}
            StmtSyntaxKind::Block(block) => self.walk_block(block, true),
            StmtSyntaxKind::Decl(declaration) => self.declare_local_variables(declaration),
            StmtSyntaxKind::Assign { left, token, .. } => {
                if *token == Token::DEFINE {
                    for expression in &**left {
                        if let ExprSyntaxKind::Ident(name) = &expression.kind {
                            self.declare(name.name.as_ref());
                        }
                    }
                }
            }
            StmtSyntaxKind::If {
                init,
                then_block,
                else_branch,
                ..
            } => {
                self.push_scope();
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                self.walk_block(then_block, true);
                if let Some(else_branch) = else_branch {
                    self.walk_stmt(else_branch);
                }
                self.pop_scope();
            }
            StmtSyntaxKind::For {
                label,
                init,
                post,
                body,
                ..
            } => {
                if let Some(label) = label {
                    self.record_label(label.name.as_ref());
                }
                self.push_scope();
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                self.walk_block(body, true);
                if let Some(post) = post {
                    self.walk_stmt(post);
                }
                self.pop_scope();
            }
            StmtSyntaxKind::Range {
                label,
                key,
                value,
                token,
                body,
                ..
            } => {
                if let Some(label) = label {
                    self.record_label(label.name.as_ref());
                }
                self.push_scope();
                if *token == Some(Token::DEFINE) {
                    for expression in key.iter().chain(value) {
                        if let ExprSyntaxKind::Ident(name) = &expression.kind {
                            self.declare(name.name.as_ref());
                        }
                    }
                }
                self.walk_block(body, true);
                self.pop_scope();
            }
            StmtSyntaxKind::Switch { init, cases, .. } => {
                self.push_scope();
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                for case in &**cases {
                    self.walk_block(&case.body, true);
                }
                self.pop_scope();
            }
            StmtSyntaxKind::TypeSwitch {
                init,
                binding,
                cases,
                ..
            } => {
                self.push_scope();
                if let Some(init) = init {
                    self.walk_stmt(init);
                }
                for case in &**cases {
                    self.push_scope();
                    if let Some(binding) = binding {
                        self.declare(binding.name.as_ref());
                    }
                    self.walk_block(&case.body, false);
                    self.pop_scope();
                }
                self.pop_scope();
            }
            StmtSyntaxKind::Select { cases } => {
                for case in &**cases {
                    self.push_scope();
                    if let Some(communication) = &case.communication {
                        self.walk_stmt(communication);
                    }
                    self.walk_block(&case.body, false);
                    self.pop_scope();
                }
            }
            StmtSyntaxKind::Labeled { label, statement } => {
                self.record_label(label.name.as_ref());
                self.walk_stmt(statement);
            }
            StmtSyntaxKind::Branch {
                token: Token::GOTO,
                label: Some(label),
            } => self.gotos.push(GotoUse {
                label: label.name.to_string(),
                source: statement.source,
                position: self.position(),
            }),
            StmtSyntaxKind::Branch { .. } => {}
        }
    }

    fn declare_local_variables(&mut self, declaration: &DeclSyntax) {
        if declaration.token != Token::VAR {
            return;
        }
        for spec in &*declaration.specs {
            for name in &*spec.names {
                self.declare(name.name.as_ref());
            }
        }
    }

    fn declare(&mut self, name: &str) {
        if name == "_" {
            return;
        }
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if scope.bindings.contains_key(name) {
            return;
        }
        let binding = self.next_binding;
        self.next_binding += 1;
        scope.bindings.insert(name.to_owned(), binding);
        self.binding_names.insert(binding, name.to_owned());
    }

    fn record_label(&mut self, label: &str) {
        let position = self.position();
        match self.labels.entry(label.to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(Some(position));
            }
            Entry::Occupied(mut entry) => {
                entry.insert(None);
            }
        }
    }

    fn position(&self) -> Position {
        Position {
            scope_path: self.scopes.iter().map(|scope| scope.id).collect(),
            bindings: self
                .scopes
                .iter()
                .flat_map(|scope| scope.bindings.values().copied())
                .collect(),
        }
    }

    fn push_scope(&mut self) {
        let id = self.next_scope;
        self.next_scope += 1;
        self.scopes.push(Scope {
            id,
            bindings: BTreeMap::new(),
        });
    }

    fn pop_scope(&mut self) {
        let popped = self.scopes.pop();
        debug_assert!(popped.is_some());
        debug_assert!(!self.scopes.is_empty());
    }
}
