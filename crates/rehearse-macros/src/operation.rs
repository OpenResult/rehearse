use crate::crate_path::runtime_crate;
use crate::diagnostics::compile_error;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    parse2, Attribute, Block, Error, FnArg, GenericArgument, Ident, ItemFn, Pat, PatIdent, PatType,
    PathArguments, ReturnType, Token, Type, TypePath, TypeReference, Visibility,
};

pub(crate) fn expand(args: TokenStream, item: TokenStream) -> TokenStream {
    match expand_result(args, item) {
        Ok(tokens) => tokens,
        Err(error) => compile_error(error),
    }
}

fn expand_result(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = parse2::<OperationArgs>(args)?;
    let input = parse2::<ItemFn>(item)?;
    let runtime = runtime_crate()?;
    let impact = parse_impact(&args.impact)?;
    let spec = OperationSpec::parse(input)?;

    Ok(spec.lower(runtime, impact))
}

struct OperationArgs {
    impact: Ident,
}

impl Parse for OperationArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error("expected `impact = pure|session|read|write|delete|opaque`"));
        }

        let key: Ident = input.parse()?;
        if key != "impact" {
            return Err(Error::new_spanned(key, "expected `impact`"));
        }

        input.parse::<Token![=]>()?;
        let impact: Ident = input.parse()?;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after operation impact"));
        }

        Ok(Self { impact })
    }
}

fn parse_impact(impact: &Ident) -> syn::Result<TokenStream> {
    match impact.to_string().as_str() {
        "pure" => Ok(quote!(Impact::Pure)),
        "session" => Ok(quote!(Impact::Session)),
        "read" => Ok(quote!(Impact::Read)),
        "write" => Ok(quote!(Impact::Write)),
        "delete" => Ok(quote!(Impact::Delete)),
        "opaque" => Ok(quote!(Impact::Opaque)),
        _ => Err(Error::new_spanned(
            impact,
            "unsupported impact; expected one of pure, session, read, write, delete, opaque",
        )),
    }
}

struct OperationSpec {
    attrs: Vec<Attribute>,
    vis: Visibility,
    name: Ident,
    context: Option<ContextParam>,
    params: Vec<OperationParam>,
    output: Type,
    error: Type,
    body: Box<Block>,
}

impl OperationSpec {
    fn parse(input: ItemFn) -> syn::Result<Self> {
        if input.sig.asyncness.is_none() {
            return Err(Error::new_spanned(
                input.sig.fn_token,
                "`#[operation]` currently supports async functions only",
            ));
        }

        if !input.sig.generics.params.is_empty() || input.sig.generics.where_clause.is_some() {
            return Err(Error::new_spanned(
                input.sig.generics,
                "generic operation functions are not supported yet",
            ));
        }

        let (output, error) = parse_result_return(&input.sig.output)?;
        let mut context = None;
        let mut params = Vec::new();

        for arg in input.sig.inputs {
            match arg {
                FnArg::Receiver(receiver) => {
                    return Err(Error::new_spanned(
                        receiver,
                        "`#[operation]` does not support methods or `self` parameters",
                    ));
                }
                FnArg::Typed(param) => {
                    let parsed = parse_param(param)?;
                    match parsed {
                        ParsedParam::Context(next_context) => {
                            if context.is_some() {
                                return Err(Error::new_spanned(
                                    next_context.ident,
                                    "`#[operation]` supports at most one `#[context]` parameter",
                                ));
                            }
                            context = Some(next_context);
                        }
                        ParsedParam::Input(param) => params.push(param),
                    }
                }
            }
        }

        if params.len() > 8 {
            return Err(Error::new(
                Span::call_site(),
                "`#[operation]` currently supports at most eight non-context parameters",
            ));
        }

        Ok(Self {
            attrs: input.attrs,
            vis: input.vis,
            name: input.sig.ident,
            context,
            params,
            output,
            error,
            body: input.block,
        })
    }

    fn lower(self, runtime: TokenStream, impact: TokenStream) -> TokenStream {
        let attrs = self.attrs;
        let vis = self.vis;
        let name = self.name;
        let operation_name = name.to_string();
        let output = self.output;
        let error = self.error;
        let body = self.body;

        let constructor_params = self.params.iter().map(|param| {
            let ident = &param.ident;
            let ty = &param.ty;
            quote!(#ident: impl #runtime::IntoInput<#ty>)
        });
        let input_conversions = self.params.iter().map(|param| {
            let ident = &param.ident;
            quote!(let #ident = #runtime::IntoInput::into_input(#ident);)
        });
        let inputs = operation_inputs(&self.params);
        let resolved_pattern = resolved_pattern(&self.params);

        let metadata = quote! {
            #runtime::OperationMetadata::new(
                #operation_name,
                #runtime::#impact,
            )
        };

        let (context_generic, context_return, context_arg, context_binding) = match self.context {
            Some(context) => {
                let context_ty = context.ty;
                let context_ident = context.ident;
                (
                    quote!(),
                    quote!(#context_ty),
                    quote!(__rehearse_context: &#context_ty),
                    quote!(let #context_ident = __rehearse_context;),
                )
            }
            None => (
                quote!(<__RehearseContext>),
                quote!(__RehearseContext),
                quote!(_rehearse_context: &__RehearseContext),
                quote!(),
            ),
        };

        let context_where = if context_generic.is_empty() {
            quote!()
        } else {
            quote!(where __RehearseContext: Sync + 'static)
        };

        quote! {
            #(#attrs)*
            #vis fn #name #context_generic(
                #(#constructor_params),*
            ) -> #runtime::Operation<#context_return, #output, #error>
            #context_where
            {
                #(#input_conversions)*

                #runtime::Operation::new(
                    #metadata,
                    #inputs,
                    move |#context_arg, #resolved_pattern| -> #runtime::BoxFuture<'_, Result<#output, #error>> {
                        Box::pin(async move {
                            #context_binding
                            #body
                        })
                    },
                )
            }
        }
    }
}

enum ParsedParam {
    Context(ContextParam),
    Input(OperationParam),
}

struct ContextParam {
    ident: Ident,
    ty: Type,
}

struct OperationParam {
    ident: Ident,
    pat: PatIdent,
    ty: Type,
}

fn parse_param(param: PatType) -> syn::Result<ParsedParam> {
    let is_context = has_context_attr(&param.attrs);
    let unsupported_attr = param
        .attrs
        .iter()
        .find(|attr| !attr.path().is_ident("context"));

    if let Some(attr) = unsupported_attr {
        return Err(Error::new_spanned(
            attr,
            "unsupported parameter attribute on `#[operation]` function",
        ));
    }

    let pat = parse_ident_pattern(&param.pat)?;

    if is_context {
        let ty = parse_context_type(&param.ty)?;
        if pat.mutability.is_some() || pat.by_ref.is_some() {
            return Err(Error::new_spanned(
                pat,
                "`#[context]` parameter must be a plain identifier",
            ));
        }

        Ok(ParsedParam::Context(ContextParam {
            ident: pat.ident,
            ty,
        }))
    } else {
        reject_borrowed_input(&param.ty)?;
        Ok(ParsedParam::Input(OperationParam {
            ident: pat.ident.clone(),
            pat,
            ty: *param.ty,
        }))
    }
}

fn has_context_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("context"))
}

fn parse_ident_pattern(pat: &Pat) -> syn::Result<PatIdent> {
    match pat {
        Pat::Ident(pat) if pat.subpat.is_none() => Ok(pat.clone()),
        _ => Err(Error::new_spanned(
            pat,
            "`#[operation]` parameters must use identifier patterns",
        )),
    }
}

fn parse_context_type(ty: &Type) -> syn::Result<Type> {
    let Type::Reference(TypeReference {
        lifetime,
        mutability,
        elem,
        ..
    }) = ty
    else {
        return Err(Error::new_spanned(
            ty,
            "`#[context]` parameter must be typed as `&Context`",
        ));
    };

    if lifetime.is_some() {
        return Err(Error::new_spanned(
            lifetime,
            "explicit lifetimes on `#[context]` parameters are not supported yet",
        ));
    }

    if mutability.is_some() {
        return Err(Error::new_spanned(
            mutability,
            "`#[context]` parameter must use an immutable reference",
        ));
    }

    Ok((**elem).clone())
}

fn reject_borrowed_input(ty: &Type) -> syn::Result<()> {
    if matches!(ty, Type::Reference(_)) {
        return Err(Error::new_spanned(
            ty,
            "borrowed non-context operation parameters are not supported yet",
        ));
    }

    Ok(())
}

fn parse_result_return(output: &ReturnType) -> syn::Result<(Type, Type)> {
    let ReturnType::Type(_, ty) = output else {
        return Err(Error::new_spanned(
            output,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    };

    let Type::Path(TypePath { path, .. }) = &**ty else {
        return Err(Error::new_spanned(
            ty,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    };

    let Some(segment) = path.segments.last() else {
        return Err(Error::new_spanned(
            ty,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    };

    if segment.ident != "Result" {
        return Err(Error::new_spanned(
            ty,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    }

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new_spanned(
            ty,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    };

    if args.args.len() != 2 {
        return Err(Error::new_spanned(
            ty,
            "`#[operation]` functions must return `Result<Output, Error>`",
        ));
    }

    let mut args = args.args.iter();
    let output = match args.next() {
        Some(GenericArgument::Type(ty)) => ty.clone(),
        _ => {
            return Err(Error::new_spanned(
                ty,
                "`#[operation]` functions must return `Result<Output, Error>`",
            ))
        }
    };
    let error = match args.next() {
        Some(GenericArgument::Type(ty)) => ty.clone(),
        _ => {
            return Err(Error::new_spanned(
                ty,
                "`#[operation]` functions must return `Result<Output, Error>`",
            ))
        }
    };

    Ok((output, error))
}

fn operation_inputs(params: &[OperationParam]) -> TokenStream {
    match params {
        [] => quote!(()),
        [one] => {
            let ident = &one.ident;
            quote!(#ident)
        }
        many => {
            let idents = many.iter().map(|param| &param.ident);
            quote!((#(#idents),*))
        }
    }
}

fn resolved_pattern(params: &[OperationParam]) -> TokenStream {
    match params {
        [] => quote!(()),
        [one] => {
            let pat = &one.pat;
            let ty = &one.ty;
            quote!(#pat: #ty)
        }
        many => {
            let pats = many.iter().map(|param| &param.pat);
            let tys = many.iter().map(|param| &param.ty);
            quote!((#(#pats),*): (#(#tys),*))
        }
    }
}
