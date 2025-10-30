CREATE SEQUENCE IF NOT EXISTS occur_sn_seq
    AS BIGINT
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

drop table if exists media_metadatas CASCADE;
CREATE TABLE media_metadatas (
    id bigserial not null PRIMARY KEY,
    media_id text NOT NULL,
    origin_server text NOT NULL,
    content_type text,
    disposition_type text,
    file_name text,
    file_extension text,
    file_size bigint NOT NULL,
    file_hash text,
    created_by text,
    created_at bigint NOT NULL
);
CREATE UNIQUE INDEX media_metadatas_udx ON media_metadatas USING btree (media_id, origin_server);

drop table if exists media_thumbnails CASCADE;
CREATE TABLE media_thumbnails (
    id bigserial not null PRIMARY KEY,
    media_id text NOT NULL,
    origin_server text NOT NULL,
    content_type text,
    disposition_type text,
    file_size bigint NOT NULL,
    width integer NOT NULL,
    height integer NOT NULL,
    resize_method text NOT NULL,
    created_at bigint NOT NULL
);
CREATE UNIQUE INDEX media_thumbnails_udx ON media_thumbnails USING btree (media_id, origin_server, width, height, resize_method);

drop table if exists user_datas CASCADE;
CREATE TABLE user_datas (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    room_id text,
    data_type text NOT NULL,
    json_data json NOT NULL,
    occur_sn bigint not null default nextval('occur_sn_seq'),
    created_at bigint NOT NULL,
    CONSTRAINT user_datas_udx UNIQUE (user_id, room_id, data_type)
);
-- CREATE UNIQUE INDEX user_datas_idx ON user_datas USING btree (user_id, room_id, data_type);

drop table if exists user_devices CASCADE;
CREATE TABLE user_devices
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    display_name text,
    user_agent text,
    is_hidden boolean DEFAULT false not null,
    last_seen_ip text,
    last_seen_at bigint,
    created_at bigint NOT NULL,
    CONSTRAINT user_devices_udx UNIQUE (device_id, user_id)
);

drop table  if exists users CASCADE;
CREATE TABLE users (
    id text NOT NULL PRIMARY KEY,
    ty text,
    is_admin boolean NOT NULL DEFAULT false,
    is_guest boolean NOT NULL DEFAULT false,
    appservice_id text,
    shadow_banned boolean NOT NULL DEFAULT false,
    consent_at bigint,
    consent_version text,
    consent_server_notice_sent text,
    approved_at bigint,
    approved_by text,
    deactivated_at bigint,
    deactivated_by text,
    locked_at bigint,
    locked_by text,
    created_at bigint NOT NULL
);


drop table if exists user_passwords CASCADE;
CREATE TABLE user_passwords (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    hash text NOT NULL,
    created_at bigint NOT NULL
);
drop table  if exists user_sessions CASCADE;
CREATE TABLE user_sessions (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    session_id text NOT NULL,
    session_type text not null,
    value json not null,
    expires_at bigint NOT NULL,
    created_at bigint NOT NULL,
    CONSTRAINT user_sessions_udx UNIQUE (user_id, session_id)
);
drop table if exists user_profiles CASCADE;
CREATE TABLE user_profiles (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    room_id text,
    display_name text,
    avatar_url text,
    blurhash text,
    CONSTRAINT user_profiles_udx UNIQUE (user_id, room_id)
);

drop table  if exists user_refresh_tokens CASCADE;
CREATE TABLE user_refresh_tokens
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    token text NOT NULL,
    next_token_id bigint,
    expires_at bigint NOT NULL,
    ultimate_session_expires_at bigint NOT NULL,
    created_at bigint NOT NULL,
    CONSTRAINT user_refresh_tokens_token_key UNIQUE (token)
);
CREATE INDEX user_refresh_tokens_next_token_id
    ON user_refresh_tokens USING btree
    (next_token_id ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE next_token_id IS NOT NULL;

drop table if exists user_access_tokens CASCADE;
CREATE TABLE user_access_tokens
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    token text NOT NULL,
    puppets_user_id text,
    last_validated bigint,
    refresh_token_id bigint,
    is_used bool not null default  false,
    expires_at bigint,
    created_at bigint not null,
    CONSTRAINT user_access_tokens_token_udx UNIQUE (user_id, device_id)
);
-- DROP INDEX IF EXISTS user_access_tokens_device_id;
-- CREATE INDEX user_access_tokens_device_id
--     ON user_access_tokens USING btree
--     (user_id ASC NULLS LAST, device_id ASC NULLS LAST);

drop table if exists room_aliases CASCADE;
CREATE TABLE room_aliases (
    alias_id text NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    created_by text NOT NULL,
    created_at bigint NOT NULL
);

drop table if exists room_tags CASCADE;
CREATE TABLE room_tags (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    room_id text NOT NULL,
    tag text NOT NULL,
    content json NOT NULL,
    created_by text NOT NULL,
    created_at bigint NOT NULL 
);
ALTER TABLE ONLY room_tags
    ADD CONSTRAINT room_tag_udx UNIQUE (user_id, room_id, tag);

drop table if exists user_openid_tokens CASCADE;
CREATE TABLE user_openid_tokens (
    id bigserial not null PRIMARY KEY,
    token text NOT NULL,
    user_id text NOT NULL,
    expires_at bigint NOT NULL,
    created_at bigint NOT NULL,
    CONSTRAINT user_openid_tokens_udx UNIQUE (token)
);
CREATE INDEX user_openid_tokens_expires_at_idx ON user_openid_tokens USING btree (expires_at);


drop table if exists user_presences CASCADE;
CREATE TABLE user_presences (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    stream_id bigint,
    state text,
    status_msg text,
    last_active_at bigint,
    last_federation_update_at bigint,
    last_user_sync_at bigint,
    currently_active boolean,
    occur_sn bigint NOT NULL default nextval('occur_sn_seq'),
    CONSTRAINT user_presences_udx UNIQUE (user_id)
);

drop table if exists threepid_guests CASCADE;
CREATE TABLE threepid_guests (
    id bigserial not null PRIMARY KEY,
    medium text,
    address text,
    access_token text,
    first_inviter text,
    created_at bigint NOT NULL
);

drop table if exists threepid_validation_sessions CASCADE;
CREATE TABLE threepid_validation_sessions (
    id bigserial not null PRIMARY KEY,
    session_id text NOT NULL,
    medium text NOT NULL,
    address text NOT NULL,
    client_secret text NOT NULL,
    last_send_attempt bigint NOT NULL,
    validated_at bigint,
    created_at bigint NOT NULL
);

drop table if exists threepid_validation_tokens CASCADE;
CREATE TABLE threepid_validation_tokens (
    id bigserial not null PRIMARY KEY,
    token text NOT NULL,
    session_id text NOT NULL,
    next_link text,
    expires_at bigint NOT NULL,
    created_at bigint NOT NULL
);

drop table if exists threepid_id_servers CASCADE;
CREATE TABLE threepid_id_servers (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    medium text NOT NULL,
    address text NOT NULL,
    id_server text NOT NULL
);

drop table if exists user_threepids CASCADE;
CREATE TABLE user_threepids (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    medium text NOT NULL,
    address text NOT NULL,
    validated_at bigint NOT NULL,
    added_at bigint NOT NULL
);

drop table if exists user_registration_tokens CASCADE;
CREATE TABLE user_registration_tokens (
    id bigserial NOT NULL PRIMARY KEY,
    token text NOT NULL,
    uses_allowed bigint,
    pending bigint NOT NULL,
    completed bigint NOT NULL,
    expires_at bigint,
    created_at bigint NOT NULL
);
ALTER TABLE ONLY user_registration_tokens
    ADD CONSTRAINT registration_tokens_token_key UNIQUE (token);

drop table if exists user_pushers CASCADE;
CREATE TABLE user_pushers (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    kind text NOT NULL,
    app_id text NOT NULL,
    app_display_name text NOT NULL,
    device_id text NOT NULL,
    device_display_name text NOT NULL,
    access_token_id bigint,
    profile_tag text,
    pushkey text NOT NULL,
    lang text NOT NULL,
    data json NOT NULL,
    enabled bool not null,
    last_stream_ordering bigint,
    last_success bigint,
    failing_since bigint,
    created_at bigint NOT NULL
);
CREATE INDEX pushers_app_id_pushkey_idx ON user_pushers USING btree (app_id, pushkey);

DROP TABLE if exists rooms CASCADE;
CREATE TABLE rooms (
    id text NOT NULL PRIMARY KEY,
    sn bigint not null default nextval('occur_sn_seq'),
    version text NOT NULL,
    is_public boolean NOT NULL default false,
    min_depth bigint not null default 0,
    state_frame_id bigint,
    has_auth_chain_index boolean not null default false,
    disabled boolean  not null default false,
    created_at bigint NOT NULL
);

drop table if exists server_signing_keys CASCADE;
CREATE TABLE server_signing_keys (
     server_id text NOT NULL PRIMARY KEY,
     key_data json NOT NULL,
     updated_at bigint NOT NULL,
     created_at bigint NOT NULL
);

drop table if exists user_filters CASCADE;
CREATE TABLE user_filters (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    filter json NOT NULL,
    created_at bigint NOT NULL
);
CREATE INDEX user_filters_user_id_idx ON user_filters USING btree (user_id);

drop table if exists user_ignores CASCADE;
CREATE TABLE user_ignores (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    ignored_id text NOT NULL,
    created_at bigint NOT NULL
);
CREATE INDEX user_ignores_user_id_idx ON user_ignores USING btree (user_id);
CREATE UNIQUE INDEX user_ignores_udx ON user_ignores USING btree (user_id, ignored_id);


drop table if exists stats_user_daily_visits CASCADE;
CREATE TABLE stats_user_daily_visits (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    user_agent text,
    created_at bigint NOT NULL
);
CREATE UNIQUE INDEX stats_user_daily_visits_user_device_ts_udx ON stats_user_daily_visits USING btree (user_id, device_id, created_at);
CREATE INDEX stats_user_daily_visits_user_ts_idx ON stats_user_daily_visits USING btree (user_id, created_at);
CREATE INDEX stats_user_daily_visits_ts_idx ON stats_user_daily_visits USING btree (created_at);


drop table if exists stats_monthly_active_users CASCADE;
CREATE TABLE stats_monthly_active_users (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    created_at bigint NOT NULL
);
CREATE INDEX monthly_active_users_ts_idx ON stats_monthly_active_users USING btree (created_at);
CREATE UNIQUE INDEX monthly_active_users_user_id_udx ON stats_monthly_active_users USING btree (user_id);


drop table if exists stats_room_currents CASCADE;
CREATE TABLE stats_room_currents (
    room_id text NOT NULL PRIMARY KEY,
    state_events bigint NOT NULL default 0,
    joined_members bigint NOT NULL default 0,
    invited_members bigint NOT NULL default 0,
    left_members bigint NOT NULL default 0,
    banned_members bigint NOT NULL default 0,
    knocked_members bigint NOT NULL default 0,
    local_users_in_room bigint NOT NULL default 0,
    completed_delta_stream_id bigint NOT NULL default 0
);

-- drop table if exists room_profiles CASCADE;
-- CREATE TABLE room_profiles (
--     id bigserial NOT NULL PRIMARY KEY,
--     room_id text NOT NULL,
--     name text,
--     canonical_alias text,
--     join_rules text,
--     history_visibility text,
--     encryption text,
--     avatar text,
--     guest_access text,
--     is_federatable boolean,
--     topic text,
--     room_kind text,
--     crated_at bigint NOT NULL
-- );

DROP TABLE IF EXISTS user_dehydrated_devices CASCADE;
CREATE TABLE user_dehydrated_devices
(
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    device_data json NOT NULL
);
CREATE INDEX IF NOT EXISTS user_dehydrated_devices_user_idx
    ON user_dehydrated_devices USING btree
    (user_id ASC NULLS LAST);


drop table if exists events CASCADE;
CREATE TABLE events (
    id text NOT NULL PRIMARY KEY,
    sn bigint NOT NULL,
    ty text NOT NULL,
    room_id text NOT NULL,
    depth bigint DEFAULT 0 NOT NULL,
    topological_ordering bigint NOT NULL,
    stream_ordering bigint NOT NULL,
    unrecognized_keys text,
    origin_server_ts bigint NOT NULL,
    received_at bigint,
    sender_id text,
    contains_url boolean NOT NULL,
    worker_id text,
    state_key text,
    is_outlier boolean NOT NULL,
    is_redacted boolean NOT NULL DEFAULT false,
    soft_failed boolean NOT NULL DEFAULT false,
    is_rejected boolean NOT NULL DEFAULT false,
    rejection_reason text,
    CONSTRAINT events_id_sn_udx UNIQUE (id, sn)
);

drop table if exists event_datas CASCADE;
CREATE TABLE event_datas
(
    event_id text NOT NULL PRIMARY KEY,
    event_sn bigint NOT NULL,
    room_id text NOT NULL,
    internal_metadata json,
    format_version bigint,
    json_data json NOT NULL,
    CONSTRAINT event_datas_udx UNIQUE (event_id, event_sn)
);

DROP TABLE IF EXISTS event_points CASCADE;
CREATE TABLE event_points
(
    event_id text NOT NULL PRIMARY KEY,
    event_sn bigint NOT NULL default nextval('occur_sn_seq'),
    room_id text NOT NULL,
    thread_id text,
    frame_id bigint,
    CONSTRAINT event_points_udx UNIQUE (event_id, event_sn)
);

drop table if exists threads CASCADE;
CREATE TABLE threads
(
    event_id text NOT NULL PRIMARY KEY,
    event_sn bigint NOT NULL,
    room_id text NOT NULL,
    last_id text NOT NULL,
    last_sn bigint NOT NULL
);
CREATE INDEX threads_event_sn_idx
    ON threads USING btree
    (room_id ASC NULLS LAST, event_sn ASC NULLS LAST);


DROP TABLE IF EXISTS room_state_frames CASCADE;
CREATE TABLE room_state_frames
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    hash_data bytea NOT NULL,
    CONSTRAINT room_state_frames_udx UNIQUE (room_id, hash_data)
);
DROP TABLE IF EXISTS room_state_fields CASCADE;
CREATE TABLE room_state_fields
(
    id bigserial NOT NULL PRIMARY KEY,
    event_ty text NOT NULL,
    state_key text NOT null,
    CONSTRAINT room_state_fields_udx UNIQUE (event_ty, state_key)
);

DROP TABLE IF EXISTS room_state_deltas CASCADE;
CREATE TABLE room_state_deltas
(
    frame_id bigint NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    parent_id bigint,
    appended bytea NOT NULL,
    disposed bytea NOT NULL
);

DROP TABLE IF EXISTS device_inboxes CASCADE;
CREATE TABLE device_inboxes
(
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    json_data json NOT NULL,
    occur_sn bigint not null default nextval('occur_sn_seq'),
    created_at bigint NOT NULL
);

CREATE INDEX device_inboxes_user_device_idx
    ON device_inboxes USING btree
    (user_id ASC NULLS LAST, device_id ASC NULLS LAST);

DROP TABLE IF EXISTS event_auth_chains CASCADE;
CREATE TABLE event_auth_chains
(
    cache_key bigint[] NOT NULL PRIMARY KEY,
    chain_sns bigint[] NOT NULL DEFAULT '{}'
);

DROP TABLE IF EXISTS event_backward_extremities CASCADE;
CREATE TABLE event_backward_extremities
(
    id bigserial NOT NULL PRIMARY KEY,
    event_id text NOT NULL,
    room_id text NOT NULL,
    CONSTRAINT event_backward_extremities_udx UNIQUE (event_id, room_id)
);
CREATE INDEX event_backward_extremities_event_id
    ON event_backward_extremities USING btree
    (event_id ASC NULLS LAST);

CREATE INDEX event_backward_extremities_room_id
    ON event_backward_extremities USING btree
    (room_id ASC NULLS LAST);


DROP TABLE IF EXISTS event_forward_extremities CASCADE;
CREATE TABLE event_forward_extremities
(
    id bigserial NOT NULL PRIMARY KEY,
    event_id text NOT NULL,
    room_id text NOT NULL,
    CONSTRAINT event_forward_extremities_udx UNIQUE (event_id, room_id)
);
CREATE INDEX event_forward_extremities_event_id
    ON event_forward_extremities USING btree
    (event_id ASC NULLS LAST);
CREATE INDEX event_forward_extremities_room_id
    ON event_forward_extremities USING btree
    (room_id ASC NULLS LAST);

DROP TABLE IF EXISTS room_users CASCADE;
CREATE TABLE room_users
(
    id bigserial NOT NULL PRIMARY KEY,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    room_id text NOT NULL,
    room_server_id text,
    user_id text NOT NULL,
    user_server_id text NOT NULL,
    sender_id text NOT NULL,
    membership text NOT NULL,
    forgotten boolean not null DEFAULT false,
    display_name text,
    avatar_url text,
    state_data json,
    created_at bigint NOT NULL,
    CONSTRAINT room_users_udx UNIQUE (event_id)
);
CREATE INDEX IF NOT EXISTS room_users_user_room_idx
    ON room_users USING btree
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST);
CREATE INDEX IF NOT EXISTS room_users_room_id_idx
    ON room_users USING btree
    (room_id ASC NULLS LAST);
CREATE INDEX IF NOT EXISTS room_users_user_id_idx
    ON room_users USING btree
    (user_id ASC NULLS LAST);
CREATE INDEX IF NOT EXISTS room_users_user_room_forgotten_idx
    ON room_users USING btree
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST)
    WHERE forgotten = true;

DROP TABLE IF EXISTS e2e_cross_signing_keys;
CREATE TABLE IF NOT EXISTS e2e_cross_signing_keys
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    key_type text NOT NULL,
    key_data json NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS e2e_cross_signing_keys_udx
    ON e2e_cross_signing_keys USING btree
    (user_id ASC NULLS LAST, key_type ASC NULLS LAST);


DROP TABLE IF EXISTS e2e_cross_signing_sigs CASCADE;
CREATE TABLE IF NOT EXISTS e2e_cross_signing_sigs
(
    id bigserial NOT NULL PRIMARY KEY,
    origin_user_id text NOT NULL,
    origin_key_id text NOT NULL,
    target_user_id text NOT NULL,
    target_device_id text NOT NULL,
    signature text NOT NULL,
    CONSTRAINT e2e_cross_signing_sigs_udx UNIQUE (origin_user_id, origin_key_id, target_user_id, target_device_id)
);
CREATE INDEX IF NOT EXISTS e2e_cross_signing_sigs_idx
    ON e2e_cross_signing_sigs USING btree
    (origin_user_id ASC NULLS LAST, target_user_id ASC NULLS LAST, target_device_id ASC NULLS LAST);

drop table if exists e2e_room_keys CASCADE;
CREATE TABLE e2e_room_keys (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    room_id text NOT NULL,
    session_id text NOT NULL,
    version bigint NOT NULL,
    first_message_index bigint,
    forwarded_count bigint,
    is_verified boolean DEFAULT false NOT NULL,
    session_data json NOT NULL,
    created_at bigint NOT NULL
);
CREATE UNIQUE INDEX  IF NOT EXISTS e2e_room_keys_udx
    ON e2e_room_keys USING btree
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST, session_id ASC NULLS LAST, version ASC NULLS LAST);


drop table if exists e2e_room_keys_versions CASCADE;
CREATE TABLE e2e_room_keys_versions (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    version bigint NOT NULL,
    algorithm json NOT NULL,
    auth_data json NOT NULL,
    is_trashed bool DEFAULT false NOT NULL,
    etag bigint NOT NULL default 0,
    created_at bigint NOT NULL
);
CREATE UNIQUE INDEX  e2e_room_keys_versions_udx ON e2e_room_keys_versions USING btree (user_id, version);


drop table if exists e2e_device_keys CASCADE;
CREATE TABLE e2e_device_keys (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    stream_id bigint NOT NULL,
    display_name text,
    key_data json NOT NULL,
    created_at bigint NOT NULL
);
ALTER TABLE ONLY e2e_device_keys
    ADD CONSTRAINT e2e_device_keys_udx UNIQUE (user_id, device_id);

drop table if exists e2e_one_time_keys CASCADE;
CREATE TABLE e2e_one_time_keys (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    algorithm text NOT NULL,
    key_id text not null,
    key_data json NOT NULL,
    created_at bigint NOT NULL,
    CONSTRAINT e2e_one_time_keys_udx UNIQUE (user_id, device_id, algorithm, key_id)
);
CREATE INDEX e2e_one_time_keys_idx ON e2e_one_time_keys USING btree (user_id, device_id);

drop table if exists e2e_fallback_keys CASCADE;
CREATE TABLE e2e_fallback_keys (
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    algorithm text NOT NULL,
    key_id text NOT NULL,
    key_data json NOT NULL,
    used_at bigint,
    created_at bigint NOT NULL
);

ALTER TABLE ONLY e2e_fallback_keys
    ADD CONSTRAINT e2e_fallback_keys_udx UNIQUE (user_id, device_id, algorithm);


DROP TABLE IF EXISTS e2e_key_changes;
CREATE TABLE IF NOT EXISTS e2e_key_changes
(
    id bigserial not null PRIMARY KEY,
    user_id text NOT NULL,
    room_id text,
    occur_sn bigint not null default nextval('occur_sn_seq'),
    changed_at bigint NOT NULL,
    CONSTRAINT e2e_key_changes_udx UNIQUE NULLS NOT DISTINCT (user_id, room_id)
);

DROP TABLE IF EXISTS user_openid_tokens;
CREATE TABLE IF NOT EXISTS user_openid_tokens
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    token text NOT NULL,
    expires_at bigint NOT NULL,
    CONSTRAINT user_openid_tokens_udx UNIQUE (token)
);

DROP TABLE IF EXISTS room_joined_servers;
CREATE TABLE IF NOT EXISTS room_joined_servers
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    server_id text NOT NULL,
    occur_sn bigint NOT NULL,
    CONSTRAINT room_joined_servers_room_id_server_id_udx UNIQUE (room_id, server_id)
);

DROP TABLE IF EXISTS event_relations;
CREATE TABLE IF NOT EXISTS event_relations
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    event_ty text NOT NULL,
    child_id text NOT NULL,
    child_sn bigint NOT NULL,
    child_ty text NOT NULL,
    rel_type text,
    CONSTRAINT event_relations_udx UNIQUE (room_id, event_id, child_id, rel_type)
);

DROP TABLE IF EXISTS event_receipts CASCADE;
CREATE TABLE event_receipts (
    id bigserial NOT NULL PRIMARY KEY,
    ty text NOT NULL,
    room_id text NOT NULL,
    user_id text NOT NULL,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    thread_id text,
    json_data json NOT NULL,
    receipt_at bigint NOT NULL
);
CREATE INDEX event_receipts_room_id_idx ON event_receipts USING btree (room_id);
CREATE INDEX event_receipts_user_id_idx ON event_receipts USING btree (user_id);
CREATE INDEX event_receipts_event_sn_idx ON event_receipts USING btree (event_sn);


DROP TABLE IF EXISTS event_searches;
CREATE TABLE IF NOT EXISTS event_searches
(
    id bigserial NOT NULL PRIMARY KEY,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    room_id text NOT NULL,
    sender_id text NOT NULL,
    key text NOT NULL,
    vector tsvector NOT NULL,
    origin_server_ts bigint NOT NULL,
    stream_ordering bigint
);

ALTER TABLE IF EXISTS event_searches
    ALTER COLUMN room_id SET (n_distinct=-0.01);

CREATE INDEX IF NOT EXISTS event_searches_ev_idx
    ON event_searches USING btree (room_id ASC NULLS LAST);
CREATE UNIQUE INDEX IF NOT EXISTS event_searches_event_id_udx
    ON event_searches USING btree (event_id ASC NULLS LAST);
CREATE INDEX IF NOT EXISTS event_search_fts_idx
    ON event_searches USING gin (vector);

ALTER TABLE ONLY event_searches
    ADD CONSTRAINT event_searches_udx UNIQUE (event_id);


DROP TABLE IF EXISTS event_push_summaries;
CREATE TABLE IF NOT EXISTS event_push_summaries
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    room_id text NOT NULL,
    notification_count bigint NOT NULL default 0,
    highlight_count bigint NOT NULL default 0,
    unread_count bigint NOT NULL default 0,
    stream_ordering bigint NOT NULL,
    thread_id text
);
CREATE INDEX IF NOT EXISTS event_push_summaries_room_id_idx
    ON event_push_summaries USING btree (room_id ASC NULLS LAST);
CREATE UNIQUE INDEX IF NOT EXISTS event_push_summaries_udx
    ON event_push_summaries USING btree
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST, thread_id ASC NULLS LAST)
WHERE thread_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS event_push_summaries_udx2
    ON event_push_summaries USING btree
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST)
WHERE thread_id IS NULL;


DROP TABLE IF EXISTS device_streams;
CREATE TABLE IF NOT EXISTS device_streams
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL
);

CREATE INDEX IF NOT EXISTS device_streams_id
    ON device_streams USING btree
    (user_id ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS device_streams_user_id
    ON device_streams USING btree
    (user_id ASC NULLS LAST, device_id ASC NULLS LAST);


DROP TABLE IF EXISTS event_edges;
CREATE TABLE IF NOT EXISTS event_edges
(
    event_id text NOT NULL PRIMARY KEY,
    prev_event_id text NOT NULL,
    room_id text,
    is_state boolean NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS event_edges_prev_id
    ON event_edges USING btree
    (prev_event_id ASC NULLS LAST);

CREATE UNIQUE INDEX IF NOT EXISTS event_edges_event_id_prev_event_id_udx
    ON event_edges USING btree
    (event_id ASC NULLS LAST, prev_event_id ASC NULLS LAST);


DROP TABLE IF EXISTS event_idempotents;
CREATE TABLE IF NOT EXISTS event_idempotents
(
    id bigserial NOT NULL PRIMARY KEY,
    txn_id text NOT NULL,
    user_id text NOT NULL,
    device_id text,
    room_id text,
    event_id text,
    created_at bigint NOT NULL
);

CREATE INDEX IF NOT EXISTS event_idempotents_created_at_idx
    ON event_idempotents USING btree
    (created_at ASC NULLS LAST);

CREATE UNIQUE INDEX IF NOT EXISTS event_idempotents_event_id_udx
    ON event_idempotents USING btree (event_id ASC NULLS LAST);
CREATE UNIQUE INDEX IF NOT EXISTS event_idempotents_txn_id_udx
    ON event_idempotents USING btree
    (txn_id ASC NULLS LAST, room_id ASC NULLS LAST, user_id ASC NULLS LAST, device_id ASC NULLS LAST);

drop table if exists lazy_load_deliveries CASCADE;
CREATE TABLE lazy_load_deliveries (
   id bigserial NOT NULL PRIMARY KEY,
   user_id text NOT NULL,
   device_id text NOT NULL,
   room_id text NOT NULL,
   confirmed_user_id text,
   CONSTRAINT lazy_loads_udx UNIQUE (user_id, device_id, room_id, confirmed_user_id)
);

drop table if exists appservice_registrations CASCADE;
CREATE TABLE appservice_registrations (
    id text NOT NULL PRIMARY KEY,
    url text,
    as_token text NOT NULL,
    hs_token text NOT NULL,
    sender_localpart text NOT NULL,
    namespaces json NOT NULL,
    rate_limited boolean,
    protocols json,
    receive_ephemeral boolean NOT NULL DEFAULT false,
    device_management boolean NOT NULL DEFAULT false
);

drop table if exists user_uiaa_datas CASCADE;
CREATE TABLE user_uiaa_datas (
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    device_id text NOT NULL,
    session text NOT NULL,
    uiaa_info json NOT NULL,
    CONSTRAINT user_uiaa_datas_udx UNIQUE (user_id, device_id, session)
);

drop table if exists outgoing_requests CASCADE;
CREATE TABLE outgoing_requests (
    id bigserial NOT NULL PRIMARY KEY,
    kind text not null,
    appservice_id text,
    user_id text,
    pushkey text,
    server_id text,
    pdu_id text,
    edu_json bytea,
    state text NOT NULL DEFAULT 'created',
    data bytea
);


drop table if exists media_url_previews CASCADE;
CREATE TABLE media_url_previews (
    id bigserial not null PRIMARY KEY,
    url text NOT NULL,
    og_title text,
    og_type text,
    og_url text,
    og_description text,
    og_image text,
    image_size bigint,
    og_image_width integer,
    og_image_height integer,
    created_at bigint NOT NULL,
    CONSTRAINT media_url_previews_ukey UNIQUE (url)
);
CREATE UNIQUE INDEX media_url_previews_index ON media_url_previews USING btree (url, og_title, og_type, og_url, og_description);


DROP TABLE IF EXISTS user_login_tokens;

CREATE TABLE IF NOT EXISTS user_login_tokens
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    token text NOT NULL,
    expires_at bigint NOT NULL,
    CONSTRAINT user_login_tokens_ukey UNIQUE (token)
);

DROP TABLE IF EXISTS room_lookup_servers;
CREATE TABLE room_lookup_servers
(
   id bigserial not null PRIMARY KEY,
   room_id text NOT NULL,
   alias_id text NOT NULL,
   server_id text NOT NULL,
   CONSTRAINT room_lookup_servers_udx UNIQUE (room_id, alias_id, server_id)
);

CREATE INDEX IF NOT EXISTS room_lookup_servers_alias_id_idx
    ON room_lookup_servers USING btree (alias_id ASC NULLS LAST);
CREATE INDEX IF NOT EXISTS room_lookup_servers_room_id_idx
    ON room_lookup_servers USING btree (room_id ASC NULLS LAST);


DROP TABLE IF EXISTS event_push_actions;
CREATE TABLE IF NOT EXISTS event_push_actions
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    user_id text NOT NULL,
    profile_tag text NOT NULL DEFAULT '',
    actions jsonb NOT NULL,
    topological_ordering bigint,
    stream_ordering bigint,
    notify boolean NOT NULL DEFAULT true,
    highlight boolean NOT NULL DEFAULT false,
    unread boolean NOT NULL DEFAULT true,
    thread_id text,
    CONSTRAINT event_push_actions_event_id_user_id_profile_tag_ukey UNIQUE (room_id, event_id, user_id, profile_tag)
);

CREATE INDEX IF NOT EXISTS event_push_actions_highlights_idx ON event_push_actions
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST, topological_ordering ASC NULLS LAST, stream_ordering ASC NULLS LAST)
    WHERE highlight = true;

CREATE INDEX IF NOT EXISTS event_push_actions_rm_tokens_idx ON event_push_actions
    (user_id ASC NULLS LAST, room_id ASC NULLS LAST, topological_ordering ASC NULLS LAST, stream_ordering ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS event_push_actions_room_id_user_id_idx ON event_push_actions
    (room_id ASC NULLS LAST, user_id ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS event_push_actions_stream_highlight_idx ON event_push_actions
    (highlight ASC NULLS LAST, stream_ordering ASC NULLS LAST) WHERE highlight = false;

CREATE INDEX IF NOT EXISTS event_push_actions_stream_ordering ON event_push_actions
    (stream_ordering ASC NULLS LAST, user_id ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS event_push_actions_u_highlight ON event_push_actions
    (user_id ASC NULLS LAST, stream_ordering ASC NULLS LAST);


DROP TABLE IF EXISTS banned_rooms;
CREATE TABLE IF NOT EXISTS banned_rooms
(
    id bigserial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    created_by text,
    created_at bigint NOT NULL,
    CONSTRAINT banned_rooms_room_id_ukey UNIQUE (room_id)
);

-- DROP TABLE IF EXISTS public.sliding_sync_connections;
-- CREATE TABLE IF NOT EXISTS public.sliding_sync_connections
-- (
--     id bigserial NOT NULL PRIMARY KEY,
--     user_id text NOT NULL,
--     device_id text NOT NULL,
--     conn_id text NOT NULL,
--     created_at bigint NOT NULL
-- );
-- CREATE INDEX IF NOT EXISTS sliding_sync_connections_idx
--     ON public.sliding_sync_connections USING btree
--     (user_id ASC NULLS LAST, device_id ASC NULLS LAST, conn_id ASC NULLS LAST);
-- CREATE INDEX IF NOT EXISTS sliding_sync_connections_create_at_idx
--     ON public.sliding_sync_connections USING btree
--     (created_at ASC NULLS LAST);

-- DROP TABLE IF EXISTS public.sliding_sync_required_states;
-- CREATE TABLE IF NOT EXISTS public.sliding_sync_required_states
-- (
--     id bigserial NOT NULL PRIMARY KEY,
--     connection_id bigint NOT NULL,
--     required_state text NOT NULL
-- );
-- CREATE INDEX IF NOT EXISTS sliding_sync_required_state_conn_pos
--     ON public.sliding_sync_required_states USING btree
--     (connection_id ASC NULLS LAST);

DROP TABLE IF EXISTS banned_rooms;
CREATE TABLE IF NOT EXISTS timeline_gaps (
    id bigserial NOT NULL PRIMARY KEY,
    room_id TEXT NOT NULL,
    event_sn BIGINT NOT NULL
);

CREATE INDEX timeline_gaps_room_id ON timeline_gaps(room_id, event_sn);
