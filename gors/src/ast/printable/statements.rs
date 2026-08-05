use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use std::io::Write;

impl<W: Write> Printable<W> for ast::ReturnStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ReturnStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Return: ")?;
        self.return_.print(p)?;

        p.prefix()?;
        p.write("Results: ")?;
        self.results.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for Vec<ast::Stmt<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Stmt (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, stmt) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                stmt.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::EmptyStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.EmptyStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Semicolon: ")?;
        self.semicolon.print(p)?;

        p.prefix()?;
        p.write("Implicit: ")?;
        self.implicit.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::AssignStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.AssignStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lhs: ")?;
        self.lhs.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.prefix()?;
        p.write("Rhs: ")?;
        self.rhs.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BlockStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BlockStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lbrace: ")?;
        self.lbrace.print(p)?;

        p.prefix()?;
        p.write("List: ")?;
        self.list.print(p)?;

        p.prefix()?;
        p.write("Rbrace: ")?;
        self.rbrace.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ExprStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ExprStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IfStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IfStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("If: ")?;
        self.if_.print(p)?;

        p.prefix()?;
        p.write("Init: ")?;
        self.init.print(p)?;

        p.prefix()?;
        p.write("Cond: ")?;
        self.cond.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.prefix()?;
        p.write("Else: ")?;
        self.else_.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IncDecStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IncDecStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::DeferStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.DeferStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Defer: ")?;
        self.defer.print(p)?;

        p.prefix()?;
        p.write("Call: ")?;
        self.call.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::GoStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.GoStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Go: ")?;
        self.go.print(p)?;

        p.prefix()?;
        p.write("Call: ")?;
        self.call.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SendStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SendStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Chan: ")?;
        self.chan.print(p)?;

        p.prefix()?;
        p.write("Arrow: ")?;
        self.arrow.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ForStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ForStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("For: ")?;
        self.for_.print(p)?;

        p.prefix()?;
        p.write("Init: ")?;
        self.init.print(p)?;

        p.prefix()?;
        p.write("Cond: ")?;
        self.cond.print(p)?;

        p.prefix()?;
        p.write("Post: ")?;
        self.post.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::RangeStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.RangeStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("For: ")?;
        self.for_.print(p)?;

        p.prefix()?;
        p.write("Key: ")?;
        self.key.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.prefix()?;
        p.write("TokPos: ")?;
        if let Some(tok_pos) = self.tok_pos {
            tok_pos.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Tok: ")?;
        if let Some(tok) = self.tok {
            tok.print(p)?;
        } else {
            p.write("ILLEGAL")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Range: ")?;
        self.range.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::DeclStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.DeclStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Decl: ")?;
        self.decl.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Stmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Stmt::AssignStmt(stmt) => stmt.print(p),
            ast::Stmt::BlockStmt(stmt) => stmt.print(p),
            ast::Stmt::BranchStmt(stmt) => stmt.print(p),
            ast::Stmt::CaseClause(stmt) => stmt.print(p),
            ast::Stmt::CommClause(stmt) => stmt.print(p),
            ast::Stmt::DeclStmt(stmt) => stmt.print(p),
            ast::Stmt::DeferStmt(stmt) => stmt.print(p),
            ast::Stmt::EmptyStmt(stmt) => stmt.print(p),
            ast::Stmt::ExprStmt(stmt) => stmt.print(p),
            ast::Stmt::ForStmt(stmt) => stmt.print(p),
            ast::Stmt::GoStmt(stmt) => stmt.print(p),
            ast::Stmt::IfStmt(stmt) => stmt.print(p),
            ast::Stmt::IncDecStmt(stmt) => stmt.print(p),
            ast::Stmt::LabeledStmt(stmt) => stmt.print(p),
            ast::Stmt::RangeStmt(stmt) => stmt.print(p),
            ast::Stmt::ReturnStmt(stmt) => stmt.print(p),
            ast::Stmt::SelectStmt(stmt) => stmt.print(p),
            ast::Stmt::SendStmt(stmt) => stmt.print(p),
            ast::Stmt::SwitchStmt(stmt) => stmt.print(p),
            ast::Stmt::TypeSwitchStmt(stmt) => stmt.print(p),
        }
    }
}

impl<W: Write> Printable<W> for ast::SwitchStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SwitchStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Switch: ")?;
        self.switch.print(p)?;

        p.prefix()?;
        p.write("Init: ")?;
        self.init.print(p)?;

        p.prefix()?;
        p.write("Tag: ")?;
        self.tag.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::TypeSwitchStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.TypeSwitchStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Switch: ")?;
        self.switch.print(p)?;

        p.prefix()?;
        p.write("Init: ")?;
        self.init.print(p)?;

        p.prefix()?;
        p.write("Assign: ")?;
        self.assign.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CaseClause<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.CaseClause ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Case: ")?;
        self.case.print(p)?;

        p.prefix()?;
        p.write("List: ")?;
        self.list.print(p)?;

        p.prefix()?;
        p.write("Colon: ")?;
        self.colon.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SelectStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SelectStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Select: ")?;
        self.select.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CommClause<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.CommClause ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Case: ")?;
        self.case.print(p)?;

        p.prefix()?;
        p.write("Comm: ")?;
        self.comm.print(p)?;

        p.prefix()?;
        p.write("Colon: ")?;
        self.colon.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BranchStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BranchStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("TokPos: ")?;
        self.tok_pos.print(p)?;

        p.prefix()?;
        p.write("Tok: ")?;
        self.tok.print(p)?;

        p.prefix()?;
        p.write("Label: ")?;
        self.label.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::LabeledStmt<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.LabeledStmt ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Label: ")?;
        self.label.print(p)?;

        p.prefix()?;
        p.write("Colon: ")?;
        self.colon.print(p)?;

        p.prefix()?;
        p.write("Stmt: ")?;
        self.stmt.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}
