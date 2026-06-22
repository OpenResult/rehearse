use std::collections::HashSet;

use syn::visit::{self, Visit};
use syn::{
    Error, Expr, ExprAsync, ExprBinary, ExprBreak, ExprCall, ExprClosure, ExprContinue,
    ExprForLoop, ExprIf, ExprMacro, ExprMatch, ExprMethodCall, ExprReference, ExprReturn, ExprTry,
    ExprUnary, ExprWhile, Ident, Macro, Path, Stmt,
};

pub(crate) fn is_step_macro(mac: &Macro) -> bool {
    path_ends_with(&mac.path, "step")
}

pub(crate) fn path_ends_with(path: &Path, ident: &str) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == ident)
}

pub(crate) fn parse_step_try(expr: &Expr) -> syn::Result<Option<Expr>> {
    let Expr::Try(ExprTry { expr, .. }) = expr else {
        return Ok(None);
    };

    let Expr::Macro(ExprMacro { mac, .. }) = &**expr else {
        return Ok(None);
    };

    if !is_step_macro(mac) {
        return Ok(None);
    }

    let operation = syn::parse2::<Expr>(mac.tokens.clone()).map_err(|_| {
        Error::new_spanned(mac, "malformed `step!`; expected `step!(operation(...))?`")
    })?;

    validate_step_operation(&operation)?;
    Ok(Some(operation))
}

pub(crate) fn has_step_macro(expr: &Expr) -> bool {
    let mut visitor = StepMacroVisitor { found: false };
    visitor.visit_expr(expr);
    visitor.found
}

pub(crate) fn has_try_expr(expr: &Expr) -> bool {
    let mut visitor = TryExprVisitor { found: false };
    visitor.visit_expr(expr);
    visitor.found
}

pub(crate) fn validate_ordinary_stmt(
    stmt: &Stmt,
    step_values: &HashSet<String>,
) -> syn::Result<()> {
    let mut visitor = OrdinaryStmtVisitor {
        step_values,
        error: None,
    };
    visitor.visit_stmt(stmt);

    if let Some(error) = visitor.error {
        Err(error)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_final_output(
    expr: &Expr,
    step_values: &HashSet<String>,
) -> syn::Result<Ident> {
    let Expr::Call(ExprCall { func, args, .. }) = expr else {
        return Err(Error::new_spanned(
            expr,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    };

    let Expr::Path(path) = &**func else {
        return Err(Error::new_spanned(
            expr,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    };

    if !path_ends_with(&path.path, "Ok") || args.len() != 1 {
        return Err(Error::new_spanned(
            expr,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    }

    let Some(arg) = args.first() else {
        return Err(Error::new_spanned(
            expr,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    };

    let Expr::Path(path) = arg else {
        return Err(Error::new_spanned(
            arg,
            "final `Ok(...)` must return a value produced by `step!`",
        ));
    };

    let Some(ident) = path.path.get_ident() else {
        return Err(Error::new_spanned(
            arg,
            "final `Ok(...)` must return a value produced by `step!`",
        ));
    };

    if !step_values.contains(&ident.to_string()) {
        return Err(Error::new_spanned(
            ident,
            "final `Ok(...)` must return a value produced by `step!`",
        ));
    }

    Ok(ident.clone())
}

fn validate_step_operation(operation: &Expr) -> syn::Result<()> {
    if has_step_macro(operation) {
        return Err(Error::new_spanned(
            operation,
            "nested `step!` calls are not supported",
        ));
    }

    if has_try_expr(operation) {
        return Err(Error::new_spanned(
            operation,
            "`?` is not supported inside `step!(...)` operation arguments",
        ));
    }

    if !matches!(operation, Expr::Call(_)) {
        return Err(Error::new_spanned(
            operation,
            "malformed `step!`; expected `step!(operation(...))?`",
        ));
    }

    Ok(())
}

struct StepMacroVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for StepMacroVisitor {
    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if is_step_macro(&node.mac) {
            self.found = true;
        }
        visit::visit_expr_macro(self, node);
    }
}

struct TryExprVisitor {
    found: bool,
}

impl<'ast> Visit<'ast> for TryExprVisitor {
    fn visit_expr_try(&mut self, _node: &'ast ExprTry) {
        self.found = true;
    }
}

struct OrdinaryStmtVisitor<'a> {
    step_values: &'a HashSet<String>,
    error: Option<Error>,
}

impl OrdinaryStmtVisitor<'_> {
    fn set_error(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn contains_step_value(&self, expr: &Expr) -> bool {
        let mut visitor = StepValueVisitor {
            step_values: self.step_values,
            found: false,
        };
        visitor.visit_expr(expr);
        visitor.found
    }
}

impl<'ast> Visit<'ast> for OrdinaryStmtVisitor<'_> {
    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if is_step_macro(&node.mac) {
            self.set_error(Error::new_spanned(
                node,
                "`step!` is only supported as a top-level pipeline statement",
            ));
            return;
        }
        visit::visit_expr_macro(self, node);
    }

    fn visit_expr_try(&mut self, node: &'ast ExprTry) {
        self.set_error(Error::new_spanned(
            node,
            "`?` is only supported immediately after `step!(...)`",
        ));
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        if has_step_macro(&node.body) {
            self.set_error(Error::new_spanned(
                node,
                "`step!` inside closures is not supported",
            ));
            return;
        }
        visit::visit_expr_closure(self, node);
    }

    fn visit_expr_async(&mut self, node: &'ast ExprAsync) {
        let mut visitor = StepMacroVisitor { found: false };
        visitor.visit_block(&node.block);
        if visitor.found {
            self.set_error(Error::new_spanned(
                node,
                "`step!` inside async blocks is not supported",
            ));
            return;
        }
        visit::visit_expr_async(self, node);
    }

    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        if self.contains_step_value(&node.cond) {
            self.set_error(Error::new_spanned(
                &node.cond,
                "branching on a value produced by `step!` is not supported",
            ));
            return;
        }
        visit::visit_expr_if(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        if self.contains_step_value(&node.expr) {
            self.set_error(Error::new_spanned(
                &node.expr,
                "matching on a value produced by `step!` is not supported",
            ));
            return;
        }
        visit::visit_expr_match(self, node);
    }

    fn visit_expr_for_loop(&mut self, node: &'ast ExprForLoop) {
        if self.contains_step_value(&node.expr) {
            self.set_error(Error::new_spanned(
                &node.expr,
                "looping over a value produced by `step!` is not supported",
            ));
            return;
        }
        visit::visit_expr_for_loop(self, node);
    }

    fn visit_expr_while(&mut self, node: &'ast ExprWhile) {
        if self.contains_step_value(&node.cond) {
            self.set_error(Error::new_spanned(
                &node.cond,
                "looping over a value produced by `step!` is not supported",
            ));
            return;
        }
        visit::visit_expr_while(self, node);
    }

    fn visit_expr_reference(&mut self, node: &'ast ExprReference) {
        if self.contains_step_value(&node.expr) {
            self.set_error(Error::new_spanned(
                node,
                "borrowing a value produced by `step!` across pipeline steps is not supported",
            ));
            return;
        }
        visit::visit_expr_reference(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.contains_step_value(&node.receiver)
            || node.args.iter().any(|arg| self.contains_step_value(arg))
        {
            self.set_error(Error::new_spanned(
                node,
                "method calls on values produced by `step!` are not supported",
            ));
            return;
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast ExprBinary) {
        if self.contains_step_value(&node.left) || self.contains_step_value(&node.right) {
            self.set_error(Error::new_spanned(
                node,
                "operators on values produced by `step!` are not supported",
            ));
            return;
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_unary(&mut self, node: &'ast ExprUnary) {
        if self.contains_step_value(&node.expr) {
            self.set_error(Error::new_spanned(
                node,
                "operators on values produced by `step!` are not supported",
            ));
            return;
        }
        visit::visit_expr_unary(self, node);
    }

    fn visit_expr_return(&mut self, node: &'ast ExprReturn) {
        self.set_error(Error::new_spanned(
            node,
            "`return` is not supported inside `#[pipeline]` bodies",
        ));
    }

    fn visit_expr_break(&mut self, node: &'ast ExprBreak) {
        self.set_error(Error::new_spanned(
            node,
            "`break` is not supported inside `#[pipeline]` bodies",
        ));
    }

    fn visit_expr_continue(&mut self, node: &'ast ExprContinue) {
        self.set_error(Error::new_spanned(
            node,
            "`continue` is not supported inside `#[pipeline]` bodies",
        ));
    }
}

struct StepValueVisitor<'a> {
    step_values: &'a HashSet<String>,
    found: bool,
}

impl<'ast> Visit<'ast> for StepValueVisitor<'_> {
    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if let Some(ident) = node.path.get_ident() {
            if self.step_values.contains(&ident.to_string()) {
                self.found = true;
            }
        }
    }
}
