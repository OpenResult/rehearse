use proc_macro2::TokenStream;
use syn::{
    parse2, Error, FnArg, GenericArgument, ItemFn, PathArguments, ReturnType, Type, TypePath,
};

pub(crate) struct PipelineSpec {
    pub(crate) item: ItemFn,
    pub(crate) context: Type,
    pub(crate) error: Type,
}

impl PipelineSpec {
    pub(crate) fn parse(item: TokenStream) -> syn::Result<Self> {
        let item = parse2::<ItemFn>(item)?;

        if item.sig.asyncness.is_some() {
            return Err(Error::new_spanned(
                item.sig.asyncness,
                "`#[pipeline]` functions must be synchronous plan constructors",
            ));
        }

        if !item.sig.generics.params.is_empty() || item.sig.generics.where_clause.is_some() {
            return Err(Error::new_spanned(
                item.sig.generics,
                "generic pipeline functions are not supported yet",
            ));
        }

        for input in &item.sig.inputs {
            if let FnArg::Receiver(receiver) = input {
                return Err(Error::new_spanned(
                    receiver,
                    "`#[pipeline]` does not support methods or `self` parameters",
                ));
            }
        }

        let (context, _output, error) = parse_plan_return(&item.sig.output)?;

        Ok(Self {
            item,
            context,
            error,
        })
    }
}

fn parse_plan_return(output: &ReturnType) -> syn::Result<(Type, Type, Type)> {
    let ReturnType::Type(_, ty) = output else {
        return Err(Error::new_spanned(
            output,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    };

    let Type::Path(TypePath { path, .. }) = &**ty else {
        return Err(Error::new_spanned(
            ty,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    };

    let Some(segment) = path.segments.last() else {
        return Err(Error::new_spanned(
            ty,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    };

    if segment.ident != "Plan" {
        return Err(Error::new_spanned(
            ty,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    }

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new_spanned(
            ty,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    };

    if args.args.len() != 3 {
        return Err(Error::new_spanned(
            ty,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        ));
    }

    let mut args = args.args.iter();
    let context = next_type_arg(args.next(), ty)?;
    let output = next_type_arg(args.next(), ty)?;
    let error = next_type_arg(args.next(), ty)?;

    Ok((context, output, error))
}

fn next_type_arg(arg: Option<&GenericArgument>, span: &Type) -> syn::Result<Type> {
    match arg {
        Some(GenericArgument::Type(ty)) => Ok(ty.clone()),
        _ => Err(Error::new_spanned(
            span,
            "`#[pipeline]` functions must return `Plan<Context, Output, Error>`",
        )),
    }
}
