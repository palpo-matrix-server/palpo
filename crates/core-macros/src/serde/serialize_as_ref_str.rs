use proc_macro2::{Ident, TokenStream};
use quote::quote;

use crate::util::NameSpace;

/// Generate the `serde::ser::Serialize` implementation for the type with the given ident, using its
/// `AsRef<str>` implementation.
pub fn expand_serialize_as_ref_str(ident: &Ident) -> syn::Result<TokenStream> {
    let palpo_core = NameSpace::palpo_core();

    Ok(quote! {
        #[automatically_derived]
        #[allow(deprecated)]
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
