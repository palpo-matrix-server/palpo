use palpo_core::MatrixError;
use salvo::prelude::*;

use crate::{AuthArgs, EmptyResult, empty_ok};
use crate::core::federation::space::{HierarchyReqArgs, HierarchyResBody};

pub fn router() -> Router {
    Router::with_path("hierarchy/{room_id}").put(get_hierarchy)
}

/// # `GET /_matrix/federation/v1/hierarchy/{roomId}`
///
/// Gets the space tree in a depth-first manner to locate child rooms of a given
/// space.
#[endpoint]
async fn get_hierarchy(_aa: AuthArgs, args: HierarchyReqArgs, depot: &mut Depot) -> JsonResult<HierarchyResBody> {
    if !crate::room::room_exists(&args.room_id)? {
		return Err(MatrixError::not_found("Room does not exist."));
	}

    let origin = depot.origin();

	let room_id = &args.room_id;
	let suggested_only = args.suggested_only;
	let ref identifier = Identifier::ServerName(origin);
	match crate::room::space::get_summary_and_children_local(room_id, identifier)?
	{
		| None => Err(MatrixError::not_found("The requested room was not found").into())

		| Some(SummaryAccessibility::Inaccessible) => {
			Err(MatrixError::not_found("The requested room is inaccessible").into())
		},

		| Some(SummaryAccessibility::Accessible(room)) => {
			let (children, inaccessible_children) =
				get_parent_children_via(&room, suggested_only)
					.filter_map(|(child, _via)| {
						match crate::room::space::get_summary_and_children_local(&child, identifier)
							.ok()?
						{
							| None => None,

							| Some(SummaryAccessibility::Inaccessible) =>
								Some((None, Some(child))),

							| Some(SummaryAccessibility::Accessible(summary)) =>
								Some((Some(summary), None)),
						}
					})
					.map(|(children, inaccessible_children): (Vec<_>, Vec<_>)| {
						(
							children.into_iter().flatten().map(Into::into).collect(),
							inaccessible_children.into_iter().flatten().collect(),
						)
					});

			json_ok(HierarchyResBody { room, children, inaccessible_children })
		},
	}
}
