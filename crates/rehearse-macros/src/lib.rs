#![forbid(unsafe_code)]

use proc_macro::TokenStream;

mod crate_path;
mod diagnostics;
mod operation;
mod pipeline;

/// Turns an async function into a delayed operation constructor.
///
/// The supported form is `#[operation(impact = pure|session|read|write|delete|opaque)]`.
#[proc_macro_attribute]
pub fn operation(args: TokenStream, item: TokenStream) -> TokenStream {
    operation::expand(args.into(), item.into()).into()
}

/// Lowers a straight-line plan constructor into `PlanBuilder` calls.
#[proc_macro_attribute]
pub fn pipeline(args: TokenStream, item: TokenStream) -> TokenStream {
    pipeline::expand(args.into(), item.into()).into()
}

/// Marker macro consumed by `#[pipeline]`.
///
/// Outside a pipeline this expands to a compile error.
#[proc_macro]
pub fn step(_input: TokenStream) -> TokenStream {
    quote::quote! {
        compile_error!("`step!` can only be used inside a `#[pipeline]` function");
    }
    .into()
}
