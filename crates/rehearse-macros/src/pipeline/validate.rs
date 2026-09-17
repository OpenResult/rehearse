use std::collections::HashSet;

use syn::visit::{self, Visit};
use syn::{Error, Expr, ExprCall, ExprMacro, ExprTry, Ident, Macro, Pat, Path, Stmt};

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
    step_values: &mut HashSet<String>,
) -> syn::Result<()> {
    let mut visitor = ScopedValidator::new(step_values.clone());
    visitor.visit_stmt(stmt);
    *step_values = visitor.step_values;
    visitor.error.map_or(Ok(()), Err)
}

/// Handles are accepted only as complete, direct operation arguments.
pub(crate) fn validate_arguments(
    operation: &Expr,
    step_values: &HashSet<String>,
) -> syn::Result<()> {
    let Expr::Call(call) = operation else {
        unreachable!("step shape was validated")
    };
    let mut visitor = ScopedValidator::new(step_values.clone());
    visitor.visit_expr(&call.func);
    for arg in &call.args {
        if let Expr::Path(path) = arg {
            if path
                .path
                .get_ident()
                .is_some_and(|ident| step_values.contains(&ident.to_string()))
            {
                continue;
            }
        }
        visitor.visit_expr(arg);
    }
    visitor.error.map_or(Ok(()), Err)
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

struct ScopedValidator {
    step_values: HashSet<String>,
    error: Option<Error>,
}

impl ScopedValidator {
    fn new(step_values: HashSet<String>) -> Self {
        Self {
            step_values,
            error: None,
        }
    }
    fn reject(&mut self, tokens: impl quote::ToTokens, message: &str) {
        if self.error.is_none() {
            self.error = Some(Error::new_spanned(tokens, message));
        }
    }
    fn shadow(&mut self, pat: &Pat) {
        struct Bindings<'a>(&'a mut HashSet<String>);
        impl<'ast> Visit<'ast> for Bindings<'_> {
            fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
                self.0.remove(&pat.ident.to_string());
                visit::visit_pat_ident(self, pat);
            }
        }
        Bindings(&mut self.step_values).visit_pat(pat);
    }
    fn condition_bindings(&mut self, expr: &Expr) {
        match expr {
            Expr::Let(expr) => self.shadow(&expr.pat),
            Expr::Binary(expr) if matches!(expr.op, syn::BinOp::And(_)) => {
                self.condition_bindings(&expr.left);
                self.condition_bindings(&expr.right);
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for ScopedValidator {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        // Ordinary items cannot capture the pipeline's local bindings. Macro
        // definitions can refer to them textually, so inspect those tokens.
        if let syn::Item::Macro(item) = node {
            self.visit_macro(&item.mac);
        }
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if node
            .path
            .get_ident()
            .is_some_and(|id| self.step_values.contains(&id.to_string()))
        {
            self.reject(
                node,
                "values produced by `step!` may only be direct step arguments or the final output",
            );
        }
        visit::visit_expr_path(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let Some(init) = &node.init {
            self.visit_expr(&init.expr);
            if let Some((_, diverge)) = &init.diverge {
                self.visit_expr(diverge);
            }
        }
        self.shadow(&node.pat);
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        let outer = self.step_values.clone();
        visit::visit_block(self, node);
        self.step_values = outer;
    }

    fn visit_expr_closure(&mut self, node: &'ast syn::ExprClosure) {
        let outer = self.step_values.clone();
        for pat in &node.inputs {
            self.shadow(pat);
        }
        self.visit_expr(&node.body);
        self.step_values = outer;
    }

    fn visit_expr_for_loop(&mut self, node: &'ast syn::ExprForLoop) {
        self.visit_expr(&node.expr);
        let outer = self.step_values.clone();
        self.shadow(&node.pat);
        self.visit_block(&node.body);
        self.step_values = outer;
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        let outer = self.step_values.clone();
        self.shadow(&node.pat);
        if let Some((_, guard)) = &node.guard {
            self.visit_expr(guard);
        }
        self.visit_expr(&node.body);
        self.step_values = outer;
    }

    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        self.visit_expr(&node.cond);
        let outer = self.step_values.clone();
        self.condition_bindings(&node.cond);
        self.visit_block(&node.then_branch);
        self.step_values = outer;
        if let Some((_, branch)) = &node.else_branch {
            self.visit_expr(branch);
        }
    }

    fn visit_expr_while(&mut self, node: &'ast syn::ExprWhile) {
        self.visit_expr(&node.cond);
        let outer = self.step_values.clone();
        self.condition_bindings(&node.cond);
        self.visit_block(&node.body);
        self.step_values = outer;
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        if is_step_macro(node) {
            self.reject(
                node,
                "`step!` is only supported as a top-level pipeline statement",
            );
        } else if tokens_reference_handle(node.tokens.clone(), &self.step_values) {
            self.reject(
                node,
                "values produced by `step!` cannot be used inside opaque macro arguments",
            );
        }
    }

    fn visit_expr_try(&mut self, node: &'ast ExprTry) {
        self.reject(node, "`?` is only supported immediately after `step!(...)`");
    }
    fn visit_expr_return(&mut self, node: &'ast syn::ExprReturn) {
        self.reject(
            node,
            "`return` is not supported inside `#[pipeline]` bodies",
        );
    }
    fn visit_expr_break(&mut self, node: &'ast syn::ExprBreak) {
        self.reject(node, "`break` is not supported inside `#[pipeline]` bodies");
    }
    fn visit_expr_continue(&mut self, node: &'ast syn::ExprContinue) {
        self.reject(
            node,
            "`continue` is not supported inside `#[pipeline]` bodies",
        );
    }
}

fn tokens_reference_handle(tokens: proc_macro2::TokenStream, handles: &HashSet<String>) -> bool {
    tokens.into_iter().any(|token| match token {
        proc_macro2::TokenTree::Ident(id) => handles.contains(&id.to_string()),
        proc_macro2::TokenTree::Group(group) => tokens_reference_handle(group.stream(), handles),
        // Also reject implicit format captures, conservatively treating words in
        // literals as potential references. We do not expand arbitrary macros.
        proc_macro2::TokenTree::Literal(literal) => literal
            .to_string()
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .any(|word| handles.contains(word)),
        proc_macro2::TokenTree::Punct(_) => false,
    })
}
