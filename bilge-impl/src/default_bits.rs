use proc_macro2::{Ident, TokenStream};
use proc_macro_error::abort_call_site;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Generics, Type, WhereClause, WherePredicate};

use crate::shared::{self, fallback::Fallback, unreachable, BitSize};

pub(crate) fn default_bits(item: TokenStream) -> TokenStream {
    let derive_input = parse(item);
    //TODO: does fallback need handling?
    let (derive_data, _, name, generics, ..) = analyze(&derive_input);

    match derive_data {
        Data::Struct(data) => generate_struct_default_impl(name, &data.fields, generics),
        Data::Enum(_) => abort_call_site!("use derive(Default) for enums"),
        _ => unreachable(()),
    }
}

fn generate_struct_default_impl(struct_name: &Ident, fields: &Fields, generics: &Generics) -> TokenStream {
    let default_value = fields
        .iter()
        .map(|field| generate_default_inner(&field.ty))
        .reduce(|acc, next| quote!(#acc | #next));

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let mut where_clause = where_clause.map(<_>::clone).unwrap_or_else(|| WhereClause {
        where_token: <_>::default(),
        predicates: <_>::default(),
    });

    // NOTE: This is not *ideal*, but it's approximately what the standard library does,
    //  for various reasons. see https://github.com/rust-lang/rust/issues/26925
    where_clause.predicates.extend(generics.type_params().map(|t| {
        let ty = &t.ident;
        let res: WherePredicate = syn::parse_quote!(#ty : ::core::fmt::Debug);
        res
    }));

    quote! {
        impl #impl_generics ::core::default::Default for #struct_name #ty_generics #where_clause {
            fn default() -> Self {
                let mut offset = 0;
                let value = #default_value;
                let value = <#struct_name #ty_generics as Bitsized>::ArbitraryInt::new(value);
                Self { value, _phantom: ::core::marker::PhantomData }
            }
        }
    }
}

fn generate_default_inner(ty: &Type) -> TokenStream {
    use Type::*;
    match ty {
        // TODO?: we could optimize nested arrays here like in `struct_gen.rs`
        // NOTE: in std, Default is only derived for arrays with up to 32 elements, but we allow more
        Array(array) => {
            let len_expr = &array.len;
            let elem_ty = &*array.elem;
            // generate the default value code for one array element
            let value_shifted = generate_default_inner(elem_ty);
            quote! {{
                // constness: iter, array::from_fn, for-loop, range are not const, so we're using while loops
                let mut acc = 0;
                let mut i = 0;
                while i < #len_expr {
                    // for every element, shift its value into its place
                    let value_shifted = #value_shifted;
                    // and bit-or them together
                    acc |= value_shifted;
                    i += 1;
                }
                acc
            }}
        }
        Path(path) => {
            let field_size = shared::generate_type_bitsize(ty);
            // u2::from(HaveFun::default()).value() as u32;
            quote! {{
                let as_int = <#path as Bitsized>::ArbitraryInt::from(<#path as ::core::default::Default>::default()).value();
                let as_base_int = as_int as <<Self as Bitsized>::ArbitraryInt as Number>::UnderlyingType;
                let shifted = as_base_int << offset;
                offset += #field_size;
                shifted
            }}
        }
        Tuple(tuple) => {
            tuple
                .elems
                .iter()
                .map(generate_default_inner)
                .reduce(|acc, next| quote!(#acc | #next))
                // `field: (),` will be handled like this:
                .unwrap_or_else(|| quote!(0))
        }
        _ => unreachable(()),
    }
}

fn parse(item: TokenStream) -> DeriveInput {
    shared::parse_derive(item)
}

fn analyze(derive_input: &DeriveInput) -> (&Data, TokenStream, &Ident, &Generics, BitSize, Option<Fallback>) {
    shared::analyze_derive(derive_input, false)
}
