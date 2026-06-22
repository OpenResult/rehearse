use crate::diagnostics::compile_error;
use proc_macro2::TokenStream;

mod lower;
mod parse;
mod validate;

pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    match expand_result(args, item) {
        Ok(tokens) => tokens,
        Err(error) => compile_error(error),
    }
}

fn expand_result(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            args,
            "`#[pipeline]` does not accept arguments",
        ));
    }

    let runtime = crate::crate_path::runtime_crate()?;
    let spec = parse::PipelineSpec::parse(item)?;
    lower::lower(spec, runtime)
}
