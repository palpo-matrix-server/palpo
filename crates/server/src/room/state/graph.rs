use std::borrow::Borrow;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt::Debug;
use std::future::Future;
use std::hash::{BuildHasher, Hash};

use crate::core::{EventId, UnixMillis};
use serde_json::from_str as from_json_str;

use crate::AppResult;

/// Sorts the event graph based on number of outgoing/incoming edges.
///
/// `key_fn` is used as to obtain the power level and age of an event for
/// breaking ties (together with the event ID).
#[tracing::instrument(level = "debug", skip_all)]
pub async fn lexicographical_topological_sort<Id, F, Fut, Hasher>(
    graph: &HashMap<Id, HashSet<Id, Hasher>>,
    key_fn: &F,
) -> AppResult<Vec<Id>>
where
    F: Fn(Id) -> Fut + Sync,
    Fut: Future<Output = AppResult<(usize, UnixMillis)>> + Send,
    Id: Borrow<EventId> + Clone + Eq + Hash + Ord + Send + Sync,
    Hasher: BuildHasher + Default + Clone + Send + Sync,
{
    #[derive(PartialEq, Eq)]
    struct TieBreaker<'a, Id> {
        power_level: usize,
        origin_server_ts: UnixMillis,
        event_id: &'a Id,
    }

    impl<Id> Ord for TieBreaker<'_, Id>
    where
        Id: Ord,
    {
        fn cmp(&self, other: &Self) -> Ordering {
            // NOTE: the power level comparison is "backwards" intentionally.
            // See the "Mainline ordering" section of the Matrix specification
            // around where it says the following:
            //
            // > for events `x` and `y`, `x < y` if [...]
            //
            // <https://spec.matrix.org/v1.12/rooms/v11/#definitions>
            other
                .power_level
                .cmp(&self.power_level)
                .then(self.origin_server_ts.cmp(&other.origin_server_ts))
                .then(self.event_id.cmp(other.event_id))
        }
    }

    impl<Id> PartialOrd for TieBreaker<'_, Id>
    where
        Id: Ord,
    {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    debug!("starting lexicographical topological sort");

    // NOTE: an event that has no incoming edges happened most recently,
    // and an event that has no outgoing edges happened least recently.

    // NOTE: this is basically Kahn's algorithm except we look at nodes with no
    // outgoing edges, c.f.
    // https://en.wikipedia.org/wiki/Topological_sorting#Kahn's_algorithm

    // outdegree_map is an event referring to the events before it, the
    // more outdegree's the more recent the event.
    let mut outdegree_map = graph.clone();

    // The number of events that depend on the given event (the EventId key)
    // How many events reference this event in the DAG as a parent
    let mut reverse_graph: HashMap<_, HashSet<_, Hasher>> = HashMap::new();

    // Vec of nodes that have zero out degree, least recent events.
    let mut zero_outdegree = Vec::new();

    for (node, edges) in graph {
        if edges.is_empty() {
            let (power_level, origin_server_ts) = key_fn(node.clone()).await?;
            // The `Reverse` is because rusts `BinaryHeap` sorts largest -> smallest we need
            // smallest -> largest
            zero_outdegree.push(Reverse(TieBreaker {
                power_level,
                origin_server_ts,
                event_id: node,
            }));
        }

        reverse_graph.entry(node).or_default();
        for edge in edges {
            reverse_graph.entry(edge).or_default().insert(node);
        }
    }

    let mut heap = BinaryHeap::from(zero_outdegree);

    // We remove the oldest node (most incoming edges) and check against all other
    let mut sorted = vec![];
    // Destructure the `Reverse` and take the smallest `node` each time
    while let Some(Reverse(item)) = heap.pop() {
        let node = item.event_id;

        for &parent in reverse_graph
            .get(node)
            .expect("EventId in heap is also in reverse_graph")
        {
            // The number of outgoing edges this node has
            let out = outdegree_map
                .get_mut(parent.borrow())
                .expect("outdegree_map knows of all referenced EventIds");

            // Only push on the heap once older events have been cleared
            out.remove(node.borrow());
            if out.is_empty() {
                let (power_level, origin_server_ts) = key_fn(parent.clone()).await?;
                heap.push(Reverse(TieBreaker {
                    power_level,
                    origin_server_ts,
                    event_id: parent,
                }));
            }
        }

        // synapse yields we push then return the vec
        sorted.push(node.clone());
    }

    Ok(sorted)
}
