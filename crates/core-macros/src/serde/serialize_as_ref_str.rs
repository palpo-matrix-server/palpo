use proc_macro2::{Ident, TokenStream};
use quote::quote;

use crate::util::import_palpo_core;

pub fn expand_serialize_as_ref_str(ident: &Ident) -> syn::Result<TokenStream> {
    let palpo_core = import_palpo_core();

    Ok(quote! {
        #[automatically_derived]
        impl #palpo_core::__private::serde::ser::Serialize for #ident {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: #palpo_core::__private::serde::ser::Serializer,
            {
                ::std::convert::AsRef::<::std::primitive::str>::as_ref(self).serialize(serializer)
            }
        }
    })
}
