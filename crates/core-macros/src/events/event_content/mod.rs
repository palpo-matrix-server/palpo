//! Implementations of the EventContent derive macro.
#![allow(clippy::too_many_arguments)] // FIXME

use std::{borrow::Cow, fmt};

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    DeriveInput, Field, Ident, LitStr, Meta, Token, Type,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
};

use self::parse::{ContentAttrs, ContentMeta, EventContentKind, EventFieldMeta, EventTypeFragment};
use super::enums::{
    EventContentTraitVariation, EventContentVariation, EventKind, EventTypes, EventVariation,
};
use crate::util::{PrivateField, m_prefix_name_to_type_name};

mod parse;

/// `EventContent` derive macro code generation.
pub fn expand_event_content(
    input: &DeriveInput,
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    let content_meta = input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("palpo_event"))
        .try_fold(ContentMeta::default(), |meta, attr| {
            let list: Punctuated<ContentMeta, Token![,]> =
                attr.parse_args_with(Punctuated::parse_terminated)?;

            list.into_iter().try_fold(meta, ContentMeta::merge)
        })?;

    let ContentAttrs {
        event_types,
        kind,
        state_key_type,
        unsigned_type
        is_custom_redacted,
        is_custom_possibly_redacted,
        has_without_relation,
    } = content_meta.try_into()?;

    let ident = &input.ident;
    let fields = match &input.data {
        syn::Data::Struct(syn::DataStruct { fields, .. }) => Some(fields.iter()),
        _ => {
            if event_kind.is_some_and(|kind| needs_redacted(is_custom_redacted, kind)) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "To generate a redacted event content, the event content type needs to be a struct. Disable this with the custom_redacted attribute",
                ));
            }

            if event_kind.is_some_and(|kind| needs_possibly_redacted(is_custom_redacted, kind)) {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "To generate a possibly redacted event content, the event content type needs to be a struct. Disable this with the custom_possibly_redacted attribute",
                ));
            }

            if has_without_relation {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "To generate an event content without relation, the event content type needs to be a struct. Disable this by removing the without_relation attribute",
                ));
            }

            None
        }
    };

    // We only generate redacted content structs for state and message-like events
    let redacted_event_content = event_kind
        .filter(|kind| needs_redacted(is_custom_redacted, *kind))
        .map(|kind| {
            generate_redacted_event_content(
                ident,
                &input.vis,
                fields.clone().unwrap(),
                &event_types,
                kind,
                state_key_type.as_ref(),
                unsigned_type.clone(),
                palpo_core,
            )
            .unwrap_or_else(syn::Error::into_compile_error)
        });

    // We only generate possibly redacted content structs for state events.
    let possibly_redacted_event_content = event_kind
        .filter(|kind| needs_possibly_redacted(is_custom_possibly_redacted, *kind))
        .map(|_| {
            generate_possibly_redacted_event_content(
                ident,
                &input.vis,
                fields.clone().unwrap(),
                &event_types,
                state_key_type.as_ref(),
                unsigned_type.clone(),
                palpo_core,
            )
            .unwrap_or_else(syn::Error::into_compile_error)
        });

    let event_content_without_relation = has_without_relation.then(|| {
        generate_event_content_without_relation(
            ident,
            &input.vis,
            fields.clone().unwrap(),
            palpo_core,
        )
        .unwrap_or_else(syn::Error::into_compile_error)
    });

    let event_content_impl = generate_event_content_impl(
        ident,
        &input.vis,
        fields,
        &event_type,
        event_kind,
        EventKindContentVariation::Original,
        state_key_type.as_ref(),
        unsigned_type,
        &aliases,
        palpo_core,
    )
    .unwrap_or_else(syn::Error::into_compile_error);
    let static_event_content_impl =
        generate_static_event_content_impl(ident, &event_type, palpo_core);
    let type_aliases = event_kind.map(|k| {
        generate_event_type_aliases(k, ident, &input.vis, &event_type.value(), palpo_core)
            .unwrap_or_else(syn::Error::into_compile_error)
    });

    Ok(quote! {
        #redacted_event_content
        #possibly_redacted_event_content
        #event_content_without_relation
        #event_content_impl
        #static_event_content_impl
        #type_aliases
    })
}

fn generate_redacted_event_content<'a>(
    ident: &Ident,
    vis: &syn::Visibility,
    fields: impl Iterator<Item = &'a Field>,
    event_types: &EventTypes,
    event_kind: EventKind,
    state_key_type: Option<&TokenStream>,
    unsigned_type: Option<TokenStream>,
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    assert!(
        !types.is_prefix(),
        "Event type shouldn't contain a `*`, this should have been checked previously"
    );

    let serde = quote! { #palpo_core::__private::serde };

    let doc = format!("Redacted form of [`{ident}`]");
    let redacted_ident = format_ident!("Redacted{ident}");

    let kept_redacted_fields: Vec<_> = fields
        .map(|f| {
            let mut keep_field = false;
            let attrs = f
                .attrs
                .iter()
                .map(|a| -> syn::Result<_> {
                    if a.path().is_ident("palpo_event") {
                        if let EventFieldMeta::SkipRedaction = a.parse_args()? {
                            keep_field = true;
                        }

                        // don't re-emit our `palpo_event` attributes
                        Ok(None)
                    } else {
                        Ok(Some(a.clone()))
                    }
                })
                .filter_map(Result::transpose)
                .collect::<syn::Result<_>>()?;

            if keep_field {
                Ok(Some(Field { attrs, ..f.clone() }))
            } else {
                Ok(None)
            }
        })
        .filter_map(Result::transpose)
        .collect::<syn::Result<_>>()?;

    let redaction_struct_fields = kept_redacted_fields.iter().flat_map(|f| &f.ident);

    let constructor = kept_redacted_fields.is_empty().then(|| {
        let doc = format!("Creates an empty {redacted_ident}.");
        quote! {
            impl #redacted_ident {
                #[doc = #doc]
                #vis fn new() -> Self {
                    Self {}
                }
            }
        }
    });

    let redacted_event_content = generate_event_content_impl(
        &redacted_ident,
        vis,
        Some(kept_redacted_fields.iter()),
        event_types,
        event_kind,
        EventKindContentVariation::Redacted,
        state_key_type,
        unsigned_type,
        palpo_core,
    )
    .unwrap_or_else(syn::Error::into_compile_error);

    let static_event_content_impl =
        generate_static_event_content_impl(&redacted_ident, event_type, palpo_core);

    Ok(quote! {
        // this is the non redacted event content's impl
        #[automatically_derived]
        impl #palpo_core::events::RedactContent for #ident {
            type Redacted = #redacted_ident;

            fn redact(self, version: &#palpo_core::RoomVersionId) -> #redacted_ident {
                #redacted_ident {
                    #( #redaction_struct_fields: self.#redaction_struct_fields, )*
                }
            }
        }

        #[doc = #doc]
        #[derive(Clone, Debug, salvo::oapi::ToSchema, #serde::Deserialize, #serde::Serialize)]
        #vis struct #redacted_ident {
            #( #kept_redacted_fields, )*
        }

        #constructor

        #redacted_event_content

        #static_event_content_impl
    })
}

fn generate_possibly_redacted_event_content<'a>(
    ident: &Ident,
    vis: &syn::Visibility,
    fields: impl Iterator<Item = &'a Field>,
    event_type: &LitStr,
    state_key_type: Option<&TokenStream>,
    unsigned_type: Option<TokenStream>,
    aliases: &[LitStr],
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    assert!(
        !event_type.value().contains('*'),
        "Event type shouldn't contain a `*`, this should have been checked previously"
    );

    let serde = quote! { #palpo_core::__private::serde };

    let doc = format!(
        "The possibly redacted form of [`{ident}`].\n\n\
        This type is used when it's not obvious whether the content is redacted or not."
    );
    let possibly_redacted_ident = format_ident!("PossiblyRedacted{ident}");

    let mut field_changed = false;
    let possibly_redacted_fields: Vec<_> = fields
        .map(|f| {
            let mut keep_field = false;
            let mut unsupported_serde_attribute = None;

            if let Type::Path(type_path) = &f.ty {
                if type_path
                    .path
                    .segments
                    .first()
                    .filter(|s| s.ident == "Option")
                    .is_some()
                {
                    // Keep the field if it's an `Option`.
                    keep_field = true;
                }
            }

            let mut attrs = f
                .attrs
                .iter()
                .map(|a| -> syn::Result<_> {
                    if a.path().is_ident("palpo_event") {
                        // Keep the field if it is not redacted.
                        if let EventFieldMeta::SkipRedaction = a.parse_args()? {
                            keep_field = true;
                        }

                        // Don't re-emit our `palpo_event` attributes.
                        Ok(None)
                    } else {
                        if a.path().is_ident("serde") {
                            if let Meta::List(list) = &a.meta {
                                let nested: Punctuated<Meta, Token![,]> =
                                    list.parse_args_with(Punctuated::parse_terminated)?;
                                for meta in &nested {
                                    if meta.path().is_ident("default") {
                                        // Keep the field if it deserializes to its default value.
                                        keep_field = true;
                                    } else if !meta.path().is_ident("rename")
                                        && !meta.path().is_ident("alias")
                                        && unsupported_serde_attribute.is_none()
                                    {
                                        // Error if the field is not kept and uses an unsupported
                                        // serde attribute.
                                        unsupported_serde_attribute =
                                            Some(syn::Error::new_spanned(
                                                meta,
                                                "Can't generate PossiblyRedacted struct with \
                                                 unsupported serde attribute\n\
                                                 Expected one of `default`, `rename` or `alias`\n\
                                                 Use the `custom_possibly_redacted` attribute \
                                                 and create the struct manually",
                                            ));
                                    }
                                }
                            }
                        }

                        Ok(Some(a.clone()))
                    }
                })
                .filter_map(Result::transpose)
                .collect::<syn::Result<_>>()?;

            if keep_field {
                Ok(Field { attrs, ..f.clone() })
            } else if let Some(err) = unsupported_serde_attribute {
                Err(err)
            } else if f.ident.is_none() {
                // If the field has no `ident`, it's a tuple struct. Since `content` is an object,
                // it will need a custom struct to deserialize from an empty object.
                Err(syn::Error::new(
                    Span::call_site(),
                    "Can't generate PossiblyRedacted struct for tuple structs\n\
                    Use the `custom_possibly_redacted` attribute and create the struct manually",
                ))
            } else {
                // Change the field to an `Option`.
                field_changed = true;

                let old_type = &f.ty;
                let ty = parse_quote! { Option<#old_type> };
                attrs.push(parse_quote! { #[serde(skip_serializing_if = "Option::is_none")] });

                Ok(Field {
                    attrs,
                    ty,
                    ..f.clone()
                })
            }
        })
        .collect::<syn::Result<_>>()?;

    // If at least one field needs to change, generate a new struct, else use a type alias.
    if field_changed {
        let possibly_redacted_event_content = generate_event_content_impl(
            &possibly_redacted_ident,
            vis,
            Some(possibly_redacted_fields.iter()),
            event_type,
            Some(EventKind::State),
            EventKindContentVariation::PossiblyRedacted,
            state_key_type,
            unsigned_type,
            aliases,
            palpo_core,
        )
        .unwrap_or_else(syn::Error::into_compile_error);

        let static_event_content_impl =
            generate_static_event_content_impl(&possibly_redacted_ident, event_type, palpo_core);

        Ok(quote! {
            #[doc = #doc]
            #[derive(salvo::oapi::ToSchema, Clone, Debug, #serde::Deserialize, #serde::Serialize)]
            #vis struct #possibly_redacted_ident {
                #( #possibly_redacted_fields, )*
            }

            #possibly_redacted_event_content

            #static_event_content_impl
        })
    } else {
        Ok(quote! {
            #[doc = #doc]
            #vis type #possibly_redacted_ident = #ident;

            #[automatically_derived]
            impl #palpo_core::events::PossiblyRedactedStateEventContent for #ident {
                type StateKey = #state_key_type;
            }
        })
    }
}

fn generate_event_content_without_relation<'a>(
    ident: &Ident,
    vis: &syn::Visibility,
    fields: impl Iterator<Item = &'a Field>,
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    let serde = quote! { #palpo_core::__private::serde };

    let type_doc = format!(
        "Form of [`{ident}`] without relation.\n\n\
        To construct this type, construct a [`{ident}`] and then use one of its `::from()` / `.into()` methods."
    );
    let without_relation_ident = format_ident!("{ident}WithoutRelation");

    let with_relation_fn_doc =
        format!("Transform `self` into a [`{ident}`] with the given relation.");

    let (relates_to, other_fields) = fields.partition::<Vec<_>, _>(|f| {
        f.ident
            .as_ref()
            .filter(|ident| *ident == "relates_to")
            .is_some()
    });

    let relates_to_type = relates_to
        .into_iter()
        .next()
        .map(|f| &f.ty)
        .ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "`without_relation` can only be used on events with a `relates_to` field",
            )
        })?;

    let without_relation_fields = other_fields
        .iter()
        .flat_map(|f| &f.ident)
        .collect::<Vec<_>>();
    let without_relation_struct = if other_fields.is_empty() {
        quote! { ; }
    } else {
        quote! {
            { #( #other_fields, )* }
        }
    };

    Ok(quote! {
        #[allow(unused_qualifications)]
        #[automatically_derived]
        impl ::std::convert::From<#ident> for #without_relation_ident {
            fn from(c: #ident) -> Self {
                Self {
                    #( #without_relation_fields: c.#without_relation_fields, )*
                }
            }
        }

        #[doc = #type_doc]
        #[derive(Clone, Debug, salvo::oapi::ToSchema, #serde::Deserialize, #serde::Serialize)]
        #vis struct #without_relation_ident #without_relation_struct

        impl #without_relation_ident {
            #[doc = #with_relation_fn_doc]
            #vis fn with_relation(self, relates_to: #relates_to_type) -> #ident {
                #ident {
                    #( #without_relation_fields: self.#without_relation_fields, )*
                    relates_to,
                }
            }
        }
    })
}

fn generate_event_type_aliases(
    event_kind: EventKind,
    ident: &Ident,
    vis: &syn::Visibility,
    event_type: &str,
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    // The redaction module has its own event types.
    if ident == "RoomRedactionEventContent" {
        return Ok(quote! {});
    }

    let ident_s = ident.to_string();
    let ev_type_s = ident_s.strip_suffix("Content").ok_or_else(|| {
        syn::Error::new_spanned(ident, "Expected content struct name ending in `Content`")
    })?;

    let type_aliases = [
        EventVariation::None,
        EventVariation::Sync,
        EventVariation::Original,
        EventVariation::OriginalSync,
        EventVariation::Stripped,
        EventVariation::Initial,
        EventVariation::Redacted,
        EventVariation::RedactedSync,
    ]
    .iter()
    .filter_map(|&var| Some((var, event_kind.to_event_ident(var).ok()?)))
    .map(|(var, ev_struct)| {
        let ev_type = format_ident!("{var}{ev_type_s}");

        let doc_text = match var {
            EventVariation::None | EventVariation::Original => "",
            EventVariation::Sync | EventVariation::OriginalSync => " from a `sync_events` response",
            EventVariation::Stripped => " from an invited room preview",
            EventVariation::Redacted => " that has been redacted",
            EventVariation::RedactedSync => " from a `sync_events` response that has been redacted",
            EventVariation::Initial => " for creating a room",
        };
        let ev_type_doc = format!("An `{event_type}` event{doc_text}.");

        let content_struct = if var.is_redacted() {
            Cow::Owned(format_ident!("Redacted{ident}"))
        } else if let EventVariation::Stripped = var {
            Cow::Owned(format_ident!("PossiblyRedacted{ident}"))
        } else {
            Cow::Borrowed(ident)
        };

        quote! {
            #[doc = #ev_type_doc]
            #vis type #ev_type = #palpo_core::events::#ev_struct<#content_struct>;
        }
    })
    .collect();

    Ok(type_aliases)
}

#[derive(PartialEq)]
enum EventKindContentVariation {
    Original,
    Redacted,
    PossiblyRedacted,
}

impl fmt::Display for EventKindContentVariation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventKindContentVariation::Original => Ok(()),
            EventKindContentVariation::Redacted => write!(f, "Redacted"),
            EventKindContentVariation::PossiblyRedacted => write!(f, "PossiblyRedacted"),
        }
    }
}

fn generate_event_content_impl<'a>(
    ident: &Ident,
    vis: &syn::Visibility,
    mut fields: Option<impl Iterator<Item = &'a Field>>,
    event_types: &EventTypes,
    event_kind: Option<EventKind>,
    variation: EventKindContentVariation,
    state_key_type: Option<&TokenStream>,
    unsigned_type: Option<TokenStream>,
    palpo_core: &TokenStream,
) -> syn::Result<TokenStream> {
    let serde = quote! { #palpo_core::__private::serde };
    let serde_json = quote! { #palpo_core::__private::serde_json };

    let (event_type_ty_decl, event_type_ty, event_type_fn_impl);

    let type_suffix_data = event_type
        .value()
        .strip_suffix('*')
        .map(|type_prefix| {
            let Some(fields) = &mut fields else {
                return Err(syn::Error::new_spanned(
                    event_type,
                    "event type with a `.*` suffix is required to be a struct",
                ));
            };

            let type_fragment_field = fields
                .find_map(|f| {
                    f.attrs
                        .iter()
                        .filter(|a| a.path().is_ident("palpo_event"))
                        .find_map(|attr| match attr.parse_args() {
                            Ok(EventFieldMeta::TypeFragment) => Some(Ok(f)),
                            Ok(_) => None,
                            Err(e) => Some(Err(e)),
                        })
                })
                .transpose()?
                .ok_or_else(|| {
                    syn::Error::new_spanned(
                        event_type,
                        "event type with a `.*` suffix requires there to be a \
                         `#[palpo_event(type_fragment)]` field",
                    )
                })?
                .ident
                .as_ref()
                .expect("type fragment field needs to have a name");

            <syn::Result<_>>::Ok((type_prefix.to_owned(), type_fragment_field))
        })
        .transpose()?;

    match event_kind {
        Some(kind) => {
            let i = kind.to_event_type_enum();
            event_type_ty_decl = None;
            event_type_ty = quote! { #palpo_core::events::#i };
            event_type_fn_impl = match &type_suffix_data {
                Some((type_prefix, type_fragment_field)) => {
                    let format = type_prefix.to_owned() + "{}";

                    quote! {
                        ::std::convert::From::from(::std::format!(#format, self.#type_fragment_field))
                    }
                }
                None => quote! { ::std::convert::From::from(#event_type) },
            };
        }
        None => {
            let camel_case_type_name = m_prefix_name_to_type_name(event_type)?;
            let i = format_ident!("{}EventType", camel_case_type_name);
            event_type_ty_decl = Some(quote! {
                /// Implementation detail, you don't need to care about this.
                #[doc(hidden)]
                #vis struct #i {
                    // Set to None for intended type, Some for a different one
                    ty: ::std::option::Option<crate::PrivOwnedStr>,
                }

                impl #serde::Serialize for #i {
                    fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
                    where
                        S: #serde::Serializer,
                    {
                        let s = self.ty.as_ref().map(|t| &t.0[..]).unwrap_or(#event_type);
                        serializer.serialize_str(s)
                    }
                }
            });
            event_type_ty = quote! { #i };
            event_type_fn_impl = quote! { #event_type_ty { ty: ::std::option::Option::None } };
        }
    }

    let sub_trait_impl = event_kind.map(|kind| {
        let trait_name = format_ident!("{variation}{kind}Content");

        let state_key = (kind == EventKind::State).then(|| {
            assert!(state_key_type.is_some());

            quote! {
                type StateKey = #state_key_type;
            }
        });

        quote! {
            #[automatically_derived]
            impl #palpo_core::events::#trait_name for #ident {
                #state_key
            }
        }
    });

    let static_state_event_content_impl = (event_kind == Some(EventKind::State)
        && variation == EventKindContentVariation::Original)
        .then(|| {
            let possibly_redacted_ident = format_ident!("PossiblyRedacted{ident}");

            let unsigned_type = unsigned_type.unwrap_or_else(
                || quote! { #palpo_core::events::StateUnsigned<Self::PossiblyRedacted> },
            );

            quote! {
                #[automatically_derived]
                impl #palpo_core::events::StaticStateEventContent for #ident {
                    type PossiblyRedacted = #possibly_redacted_ident;
                    type Unsigned = #unsigned_type;
                }
            }
        });

    let event_types = aliases.iter().chain([event_type]);

    let event_content_from_type_impl = if let Some((_, type_fragment_field)) = &type_suffix_data {
        let type_prefixes = event_types.map(|ev_type| {
            ev_type
                .value()
                .strip_suffix('*')
                .expect("aliases have already been checked to have the same suffix")
                .to_owned()
        });
        let type_prefixes = quote! {
            [#(#type_prefixes,)*]
        };
        let fields_without_type_fragment = fields
            .unwrap()
            .filter(|f| {
                !f.attrs.iter().any(|a| {
                    a.path().is_ident("palpo_event")
                        && matches!(a.parse_args(), Ok(EventFieldMeta::TypeFragment))
                })
            })
            .map(PrivateField)
            .collect::<Vec<_>>();
        let fields_ident_without_type_fragment = fields_without_type_fragment
            .iter()
            .filter_map(|f| f.0.ident.as_ref());

        quote! {
            impl #palpo_core::events::EventContentFromType for #ident {
                fn from_parts(
                    ev_type: &::std::primitive::str,
                    content: &#serde_json::value::RawValue,
                ) -> #serde_json::Result<Self> {
                    #[derive(#serde::Deserialize)]
                    struct WithoutTypeFragment {
                        #( #fields_without_type_fragment, )*
                    }

                    if let ::std::option::Option::Some(type_fragment) =
                        #type_prefixes.iter().find_map(|prefix| ev_type.strip_prefix(prefix))
                    {
                        let c: WithoutTypeFragment = #serde_json::from_str(content.get())?;

                        ::std::result::Result::Ok(Self {
                            #(
                                #fields_ident_without_type_fragment:
                                    c.#fields_ident_without_type_fragment,
                            )*
                            #type_fragment_field: type_fragment.to_owned(),
                        })
                    } else {
                        ::std::result::Result::Err(#serde::de::Error::custom(
                            ::std::format!(
                                "expected event type starting with one of `{:?}`, found `{}`",
                                #type_prefixes, ev_type,
                            )
                        ))
                    }
                }
            }
        }
    } else {
        quote! {
            impl #palpo_core::events::EventContentFromType for #ident {
                fn from_parts(
                    ev_type: &::std::primitive::str,
                    content: &#serde_json::value::RawValue,
                ) -> #serde_json::Result<Self> {
                    #serde_json::from_str(content.get())
                }
            }
        }
    };

    Ok(quote! {
        #event_type_ty_decl

        #[automatically_derived]
        impl #palpo_core::events::EventContent for #ident {
            type EventType = #event_type_ty;

            fn event_type(&self) -> Self::EventType {
                #event_type_fn_impl
            }
        }

        #event_content_from_type_impl
        #sub_trait_impl
        #static_state_event_content_impl
    })
}

fn generate_static_event_content_impl(
    ident: &Ident,
    event_types: &EventTypes,
    palpo_core: &TokenStream,
) -> TokenStream {
    let event_type = event_types.event_type.without_wildcard();
    let static_event_type = quote! { #event_type };

    let is_prefix = if types.is_prefix() {
        quote! { #palpo_core::events::True }
    } else {
        quote! { #palpo_core::events::False }
    };

    quote! {
        impl #palpo_core::events::StaticEventContent for #ident {
            const TYPE: &'static ::std::primitive::str = #event_type;
            type IsPrefix = #is_prefix;
        }
    }
}

fn needs_redacted(is_custom_redacted: bool, event_kind: EventKind) -> bool {
    // `is_custom` means that the content struct does not need a generated
    // redacted struct also. If no `custom_redacted` attrs are found the content
    // needs a redacted struct generated.
    !is_custom_redacted && matches!(event_kind, EventKind::MessageLike | EventKind::State)
}

fn needs_possibly_redacted(is_custom_possibly_redacted: bool, event_kind: EventKind) -> bool {
    // `is_custom_possibly_redacted` means that the content struct does not need
    // a generated possibly redacted struct also. If no `custom_possibly_redacted`
    // attrs are found the content needs a possibly redacted struct generated.
    !is_custom_possibly_redacted && event_kind == EventKind::State
}
