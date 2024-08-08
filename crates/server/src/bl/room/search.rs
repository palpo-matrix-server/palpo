use diesel::prelude::*;

use crate::core::identifiers::*;
use crate::schema::*;
use crate::{utils, AppResult, PduEvent};

pub fn search_pdus(room_id: &RoomId, search_string: &str) -> AppResult<Option<(Vec<OwnedEventId>, Vec<String>)>> {
    let words: Vec<_> = search_string
        .split_terminator(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect();

    // TODO: fixme
    panic!("search_pdus Not implemented")
    // let iterators = words.clone().into_iter().map(move |word| {
    //     let mut prefix2 = prefix.clone();
    //     prefix2.extend_from_slice(word.as_bytes());
    //     prefix2.push(0xff);
    //     let prefix3 = prefix2.clone();

    //     let mut last_possible_id = prefix2.clone();
    //     last_possible_id.extend_from_slice(&u64::MAX.to_be_bytes());

    //     self.tokenids
    //         .iter_from(&last_possible_id, true) // Newest pdus first
    //         .take_while(move |(k, _)| k.starts_with(&prefix2))
    //         .map(move |(key, _)| key[prefix3.len()..].to_vec())
    // });

    // let common_elements = match utils::common_elements(iterators, |a, b| {
    //     // We compare b with a because we reversed the iterator earlier
    //     b.cmp(a)
    // }) {
    //     Some(it) => it,
    //     None => return Ok(None),
    // };

    // Ok(Some((Box::new(common_elements), words)))
}
