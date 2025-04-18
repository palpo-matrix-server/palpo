DROP TABLE IF EXISTS user_login_tokens;

CREATE TABLE IF NOT EXISTS user_login_tokens
(
    id bigserial NOT NULL PRIMARY KEY,
    user_id text NOT NULL,
    token text NOT NULL,
    expires_at bigint NOT NULL,
    CONSTRAINT user_login_tokens_ukey UNIQUE (token)
);