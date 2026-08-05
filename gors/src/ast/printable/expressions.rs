use super::print_go_string;
use crate::ast;
use crate::ast::printer::{PrintResult, Printable, Printer};
use std::io::Write;

impl<W: Write> Printable<W> for Vec<ast::Ident<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]*ast.Ident (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, ident) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                ident.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BinaryExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BinaryExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("OpPos: ")?;
        self.op_pos.print(p)?;

        p.prefix()?;
        p.write("Op: ")?;
        self.op.print(p)?;

        p.prefix()?;
        p.write("Y: ")?;
        self.y.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::BasicLit<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.BasicLit ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("ValuePos: ")?;
        self.value_pos.print(p)?;

        p.prefix()?;
        p.write("ValueEnd: ")?;
        self.value_end.print(p)?;

        p.prefix()?;
        p.write("Kind: ")?;
        self.kind.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        // Print string using Go-compatible escape format (use \uXXXX instead of \u{XXXX})
        print_go_string(&mut p.w, self.value)?;
        p.newline()?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Ellipsis<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Ellipsis ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Ellipsis: ")?;
        self.ellipsis.print(p)?;

        p.prefix()?;
        p.write("Elt: ")?;
        self.elt.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Ident<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.Ident ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("NamePos: ")?;
        self.name_pos.print(p)?;

        p.prefix()?;
        write!(p.w, "Name: {:?}", self.name)?;
        p.newline()?;

        p.prefix()?;
        p.write("Obj: ")?;
        self.obj.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::StarExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.StarExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Star: ")?;
        self.star.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::UnaryExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.UnaryExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("OpPos: ")?;
        self.op_pos.print(p)?;

        p.prefix()?;
        p.write("Op: ")?;
        self.op.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CallExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.CallExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Fun: ")?;
        self.fun.print(p)?;

        p.prefix()?;
        p.write("Lparen: ")?;
        self.lparen.print(p)?;

        p.prefix()?;
        p.write("Args: ")?;
        self.args.print(p)?;

        p.prefix()?;
        p.write("Ellipsis: ")?;
        if let Some(ellipsis) = self.ellipsis {
            ellipsis.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Rparen: ")?;
        self.rparen.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IndexExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IndexExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Lbrack: ")?;
        self.lbrack.print(p)?;

        p.prefix()?;
        p.write("Index: ")?;
        self.index.print(p)?;

        p.prefix()?;
        p.write("Rbrack: ")?;
        self.rbrack.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::IndexListExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.IndexListExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Lbrack: ")?;
        self.lbrack.print(p)?;

        p.prefix()?;
        p.write("Indices: ")?;
        self.indices.print(p)?;

        p.prefix()?;
        p.write("Rbrack: ")?;
        self.rbrack.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ParenExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ParenExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lparen: ")?;
        self.lparen.print(p)?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Rparen: ")?;
        self.rparen.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SelectorExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SelectorExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Sel: ")?;
        self.sel.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ChanType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ChanType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Begin: ")?;
        self.begin.print(p)?;

        p.prefix()?;
        p.write("Arrow: ")?;
        if let Some(arrow) = self.arrow {
            arrow.print(p)?;
        } else {
            p.write("-")?;
            p.newline()?;
        }

        p.prefix()?;
        p.write("Dir: ")?;
        self.dir.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::MapType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.MapType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Map: ")?;
        self.map.print(p)?;

        p.prefix()?;
        p.write("Key: ")?;
        self.key.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::CompositeLit<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.CompositeLit ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Lbrace: ")?;
        self.lbrace.print(p)?;

        p.prefix()?;
        p.write("Elts: ")?;
        self.elts.print(p)?;

        p.prefix()?;
        p.write("Rbrace: ")?;
        self.rbrace.print(p)?;

        p.prefix()?;
        p.write("Incomplete: ")?;
        self.incomplete.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::TypeAssertExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.TypeAssertExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Lparen: ")?;
        self.lparen.print(p)?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Rparen: ")?;
        self.rparen.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::KeyValueExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.KeyValueExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Key: ")?;
        self.key.print(p)?;

        p.prefix()?;
        p.write("Colon: ")?;
        self.colon.print(p)?;

        p.prefix()?;
        p.write("Value: ")?;
        self.value.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::SliceExpr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.SliceExpr ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("X: ")?;
        self.x.print(p)?;

        p.prefix()?;
        p.write("Lbrack: ")?;
        self.lbrack.print(p)?;

        p.prefix()?;
        p.write("Low: ")?;
        self.low.print(p)?;

        p.prefix()?;
        p.write("High: ")?;
        self.high.print(p)?;

        p.prefix()?;
        p.write("Max: ")?;
        self.max.print(p)?;

        p.prefix()?;
        p.write("Slice3: ")?;
        self.slice3.print(p)?;

        p.prefix()?;
        p.write("Rbrack: ")?;
        self.rbrack.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::FuncLit<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.FuncLit ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Type: ")?;
        self.type_.print(p)?;

        p.prefix()?;
        p.write("Body: ")?;
        self.body.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::ArrayType<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        p.write("*ast.ArrayType ")?;
        p.open_bracket()?;

        p.prefix()?;
        p.write("Lbrack: ")?;
        self.lbrack.print(p)?;

        p.prefix()?;
        p.write("Len: ")?;
        self.len.print(p)?;

        p.prefix()?;
        p.write("Elt: ")?;
        self.elt.print(p)?;

        p.close_bracket()?;

        Ok(())
    }
}

impl<W: Write> Printable<W> for ast::Expr<'_> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        match self {
            ast::Expr::ArrayType(node) => node.print(p),
            ast::Expr::BasicLit(node) => node.print(p),
            ast::Expr::BinaryExpr(node) => node.print(p),
            ast::Expr::CallExpr(node) => node.print(p),
            ast::Expr::ChanType(node) => node.print(p),
            ast::Expr::CompositeLit(node) => node.print(p),
            ast::Expr::Ellipsis(node) => node.print(p),
            ast::Expr::FuncLit(node) => node.print(p),
            ast::Expr::FuncType(node) => node.print(p),
            ast::Expr::Ident(node) => node.print(p),
            ast::Expr::IndexExpr(node) => node.print(p),
            ast::Expr::IndexListExpr(node) => node.print(p),
            ast::Expr::InterfaceType(node) => node.print(p),
            ast::Expr::KeyValueExpr(node) => node.print(p),
            ast::Expr::MapType(node) => node.print(p),
            ast::Expr::ParenExpr(node) => node.print(p),
            ast::Expr::SelectorExpr(node) => node.print(p),
            ast::Expr::SliceExpr(node) => node.print(p),
            ast::Expr::StarExpr(node) => node.print(p),
            ast::Expr::StructType(node) => node.print(p),
            ast::Expr::TypeAssertExpr(node) => node.print(p),
            ast::Expr::UnaryExpr(node) => node.print(p),
        }
    }
}

impl<W: Write> Printable<W> for Vec<ast::Expr<'_>> {
    fn print(&self, p: &mut Printer<W>) -> PrintResult {
        if self.is_empty() {
            p.write("nil")?;
            p.newline()?;
        } else {
            write!(p.w, "[]ast.Expr (len = {}) ", self.len())?;
            p.open_bracket()?;
            for (i, expr) in self.iter().enumerate() {
                p.prefix()?;
                write!(p.w, "{}: ", i)?;
                expr.print(p)?;
            }
            p.close_bracket()?;
        }
        Ok(())
    }
}
