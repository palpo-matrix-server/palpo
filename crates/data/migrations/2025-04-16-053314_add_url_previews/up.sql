
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

ALTER TABLE IF EXISTS public.events
    ALTER COLUMN origin_server_ts SET NOT NULL;