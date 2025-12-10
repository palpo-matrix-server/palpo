DROP TABLE IF EXISTS event_edges;
CREATE TABLE event_edges
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    prev_id text NOT NULL
);
CREATE INDEX event_edges_prev_id_idx
    ON event_edges USING btree
    (prev_id ASC NULLS LAST)
    WITH (fillfactor=100, deduplicate_items=True);
CREATE UNIQUE INDEX event_edges_event_id_prev_id_idx
    ON event_edges USING btree
    (event_id ASC NULLS LAST, prev_id ASC NULLS LAST)
    WITH (fillfactor=100, deduplicate_items=True);