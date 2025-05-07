
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

ALTER TABLE IF EXISTS public.room_servers
    RENAME TO room_joined_servers;
ALTER TABLE IF EXISTS public.room_joined_servers
    ADD COLUMN occur_sn bigint NOT NULL DEFAULT 0;