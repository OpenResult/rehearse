use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Error, Expr, Local, Pat, Stmt};

use super::parse::PipelineSpec;
use super::validate;

pub(crate) fn lower(spec: PipelineSpec, runtime: TokenStream) -> syn::Result<TokenStream> {
    let PipelineSpec {
        item,
        context,
        error,
    } = spec;
    let attrs = item.attrs;
    let vis = item.vis;
    let sig = item.sig;
    let name = sig.ident.to_string();
    let builder = format_ident!("__rehearse_builder", span = Span::mixed_site());
    let block = item.block;
    let stmts = block.stmts;
    let mut step_values = HashSet::new();
    let mut statements = Vec::new();

    let Some((last, prefix)) = stmts.split_last() else {
        return Err(Error::new_spanned(
            &sig.ident,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    };

    for stmt in prefix {
        if let Some(lowered_step) = parse_step_stmt(stmt, &mut step_values)? {
            statements.push(lowered_step);
        } else {
            validate::validate_ordinary_stmt(stmt, &mut step_values)?;
            statements.push(Statement::Ordinary(stmt.clone()));
        }
    }

    let output = match last {
        Stmt::Expr(expr, None) => validate::validate_final_output(expr, &step_values)?,
        _ => {
            return Err(Error::new_spanned(
                last,
                "`#[pipeline]` functions must end with `Ok(value)`",
            ))
        }
    };

    let lowered = statements.iter().map(|statement| statement.lower(&builder));
    Ok(quote! {
        #(#attrs)*
        #vis #sig {
            let mut #builder = #runtime::PlanBuilder::<#context, #error>::new(#name);
            #(#lowered)*
            #builder.finish(#output)
        }
    })
}

fn parse_step_stmt(
    stmt: &Stmt,
    step_values: &mut HashSet<String>,
) -> syn::Result<Option<Statement>> {
    match stmt {
        Stmt::Local(local) => parse_local_step(local, step_values),
        Stmt::Expr(expr, Some(_semi)) => parse_bare_step(expr, step_values),
        Stmt::Expr(expr, None) => {
            if validate::has_step_macro(expr) {
                return Err(Error::new_spanned(
                    expr,
                    "`step!(...)` must be followed by `?` and cannot be the final expression",
                ));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn parse_local_step(
    local: &Local,
    step_values: &mut HashSet<String>,
) -> syn::Result<Option<Statement>> {
    let Some(init) = &local.init else {
        return Ok(None);
    };

    if validate::has_step_macro(&init.expr) && !matches!(&*init.expr, Expr::Try(_)) {
        if let Expr::Closure(_) = &*init.expr {
            return Err(Error::new_spanned(
                &init.expr,
                "`step!` inside closures is not supported",
            ));
        }

        if let Expr::Async(_) = &*init.expr {
            return Err(Error::new_spanned(
                &init.expr,
                "`step!` inside async blocks is not supported",
            ));
        }

        return Err(Error::new_spanned(
            &init.expr,
            "`step!(...)` in a let binding must be followed by `?`",
        ));
    }

    let Some(operation) = validate::parse_step_try(&init.expr)? else {
        return Ok(None);
    };

    if init.diverge.is_some() {
        return Err(Error::new_spanned(
            &init.expr,
            "`else` blocks on `step!(...)` bindings are not supported",
        ));
    }

    let Pat::Ident(pat) = &local.pat else {
        return Err(Error::new_spanned(
            &local.pat,
            "`step!(...)` bindings must use a plain identifier pattern",
        ));
    };

    if pat.subpat.is_some() || pat.by_ref.is_some() || pat.mutability.is_some() {
        return Err(Error::new_spanned(
            pat,
            "`step!(...)` bindings must use a plain identifier pattern",
        ));
    }

    validate::validate_arguments(&operation, step_values)?;
    let ident = &pat.ident;
    step_values.insert(ident.to_string());

    Ok(Some(Statement::Step {
        binding: Some(ident.clone()),
        operation,
    }))
}

fn parse_bare_step(expr: &Expr, step_values: &HashSet<String>) -> syn::Result<Option<Statement>> {
    if validate::has_step_macro(expr) && !matches!(expr, Expr::Try(_)) {
        return Err(Error::new_spanned(
            expr,
            "`step!(...)` statements must be followed by `?`",
        ));
    }

    let Some(operation) = validate::parse_step_try(expr)? else {
        return Ok(None);
    };

    validate::validate_arguments(&operation, step_values)?;
    Ok(Some(Statement::Step {
        binding: None,
        operation,
    }))
}

// Emit code only after every statement and the final output have been validated.
enum Statement {
    Ordinary(Stmt),
    Step {
        binding: Option<syn::Ident>,
        operation: Expr,
    },
}

impl Statement {
    fn lower(&self, builder: &syn::Ident) -> TokenStream {
        match self {
            Self::Ordinary(stmt) => quote!(#stmt),
            Self::Step {
                binding: Some(binding),
                operation,
            } => quote!(let #binding = #builder.add(#operation);),
            Self::Step {
                binding: None,
                operation,
            } => quote!(let _ = #builder.add(#operation);),
        }
    }
}
