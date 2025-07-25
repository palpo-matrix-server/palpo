use std::collections::HashMap;

use syn::{Expr, ExprLit, Lit, Meta, MetaNameValue};

// pub(crate) fn get_crate_name() -> Option<String> {
//     let cargo_crate_name = std::env::var("CARGO_CRATE_NAME");
//     match cargo_crate_name.as_ref() {
//         Err(_) => None,
//         Ok(crate_name) => Some(crate_name.trim_start_matches("palpo_").to_owned()),
//     }
// }

pub(crate) fn get_simple_settings(args: &[Meta]) -> HashMap<String, String> {
    args.iter().fold(HashMap::new(), |mut map, arg| {
        let Meta::NameValue(MetaNameValue { path, value, .. }) = arg else {
            return map;
        };

        let Expr::Lit(ExprLit { lit: Lit::Str(str), .. }, ..) = value else {
            return map;
        };

        if let Some(key) = path.segments.iter().next().map(|s| s.ident.clone()) {
            map.insert(key.to_string(), str.value());
        }

        map
    })
}

pub(crate) fn is_cargo_build() -> bool {
    legacy_is_cargo_build()
        || std::env::args()
            .skip_while(|flag| !flag.starts_with("--emit"))
            .nth(1)
            .iter()
            .flat_map(|flag| flag.split(','))
            .any(|elem| elem == "link")
}

pub(crate) fn legacy_is_cargo_build() -> bool {
    std::env::args()
        .find(|flag| flag.starts_with("--emit"))
        .as_ref()
        .and_then(|flag| flag.split_once('='))
        .map(|val| val.1.split(','))
        .and_then(|mut vals| vals.find(|elem| *elem == "link"))
        .is_some()
}

pub(crate) fn is_cargo_test() -> bool {
    std::env::args().any(|flag| flag == "--test")
}

#[inline]
pub(crate) fn exchange<T>(state: &mut T, source: T) -> T {
    std::mem::replace(state, source)
}
