use std::collections::HashSet;

use proc_macro2::TokenStream;
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
    let builder = format_ident!("__rehearse_builder");
    let block = item.block;
    let stmts = block.stmts;
    let mut step_values = HashSet::new();
    let mut lowered = Vec::new();

    let Some((last, prefix)) = stmts.split_last() else {
        return Err(Error::new_spanned(
            &sig.ident,
            "`#[pipeline]` functions must end with `Ok(value)`",
        ));
    };

    for stmt in prefix {
        if let Some(lowered_step) = lower_step_stmt(stmt, &builder, &mut step_values)? {
            lowered.push(lowered_step);
        } else {
            validate::validate_ordinary_stmt(stmt, &step_values)?;
            lowered.push(quote!(#stmt));
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

    Ok(quote! {
        #(#attrs)*
        #vis #sig {
            let mut #builder = #runtime::PlanBuilder::<#context, #error>::new(#name);
            #(#lowered)*
            #builder.finish(#output)
        }
    })
}

fn lower_step_stmt(
    stmt: &Stmt,
    builder: &proc_macro2::Ident,
    step_values: &mut HashSet<String>,
) -> syn::Result<Option<TokenStream>> {
    match stmt {
        Stmt::Local(local) => lower_local_step(local, builder, step_values),
        Stmt::Expr(expr, Some(_semi)) => lower_bare_step(expr, builder),
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

fn lower_local_step(
    local: &Local,
    builder: &proc_macro2::Ident,
    step_values: &mut HashSet<String>,
) -> syn::Result<Option<TokenStream>> {
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

    let ident = &pat.ident;
    step_values.insert(ident.to_string());

    Ok(Some(quote! {
        let #ident = #builder.add(#operation);
    }))
}

fn lower_bare_step(expr: &Expr, builder: &proc_macro2::Ident) -> syn::Result<Option<TokenStream>> {
    if validate::has_step_macro(expr) && !matches!(expr, Expr::Try(_)) {
        return Err(Error::new_spanned(
            expr,
            "`step!(...)` statements must be followed by `?`",
        ));
    }

    let Some(operation) = validate::parse_step_try(expr)? else {
        return Ok(None);
    };

    Ok(Some(quote! {
        let _ = #builder.add(#operation);
    }))
}
