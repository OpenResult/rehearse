use proc_macro::TokenStream;

mod crate_path;
mod diagnostics;
mod operation;

#[proc_macro_attribute]
pub fn operation(args: TokenStream, item: TokenStream) -> TokenStream {
    operation::expand(args.into(), item.into()).into()
}
