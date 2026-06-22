use proc_macro2::TokenStream;

pub(crate) fn compile_error(error: syn::Error) -> TokenStream {
    error.to_compile_error()
}
