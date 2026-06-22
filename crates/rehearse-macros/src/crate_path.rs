use proc_macro2::{Span, TokenStream};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{format_ident, quote};

pub(crate) fn runtime_crate() -> syn::Result<TokenStream> {
    match crate_name("rehearse") {
        Ok(FoundCrate::Itself) => Ok(quote!(::rehearse)),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name);
            Ok(quote!(::#ident))
        }
        Err(error) => Err(syn::Error::new(
            Span::call_site(),
            format!("could not locate `rehearse` crate: {error}"),
        )),
    }
}
