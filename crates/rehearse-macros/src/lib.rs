use proc_macro::TokenStream;

mod crate_path;
mod diagnostics;
mod operation;
mod pipeline;

#[proc_macro_attribute]
pub fn operation(args: TokenStream, item: TokenStream) -> TokenStream {
    operation::expand(args.into(), item.into()).into()
}

#[proc_macro_attribute]
pub fn pipeline(args: TokenStream, item: TokenStream) -> TokenStream {
    pipeline::expand(args.into(), item.into()).into()
}

#[proc_macro]
pub fn step(_input: TokenStream) -> TokenStream {
    quote::quote! {
        compile_error!("`step!` can only be used inside a `#[pipeline]` function");
    }
    .into()
}
