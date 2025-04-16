
drop table if exists media_url_previews CASCADE;
CREATE TABLE media_url_previews (
    id bigserial not null PRIMARY KEY,
    url text NOT NULL,
    title text,
    description text,
    image text,
    image_size bigint,
    image_width integer,
    image_height integer,
    created_at bigint NOT NULL,
    CONSTRAINT media_url_previews_ukey UNIQUE (url)
);
CREATE UNIQUE INDEX media_url_previews_index ON media_url_previews USING btree (url, title, description);