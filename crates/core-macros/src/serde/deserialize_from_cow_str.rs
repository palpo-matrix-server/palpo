use proc_macro2::{Ident, TokenStream};
use quote::quote;

use crate::util::NameSpace;

/// Generate the `serde::de:Deserialize` implementation for the type with the given ident, using its
/// `From<Cow<'a, str>>` implementation.
pub fn expand_deserialize_from_cow_str(ident: &Ident) -> syn::Result<TokenStream> {
    let palpo_core = NameSpace::palpo_core();
    let serde = NameSpace::serde();

    Ok(quote! {
        #[automatically_derived]
        #[allow(deprecated)]
        impl<'de> #serde::de::Deserialize<'de> for #ident {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: #serde::de::Deserializer<'de>,
            {
                type CowStr<'a> = ::std::borrow::Cow<'a, ::std::primitive::str>;

                let cow = #palpo_core::serde::deserialize_cow_str(deserializer)?;
                Ok(::std::convert::From::<CowStr<'_>>::from(cow))
            }
        }
    })
}
