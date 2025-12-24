-- Add table for cross-signing key replacement UIA bypass
-- This stores the timestamp until which a user can replace their cross-signing keys without UIA
CREATE TABLE e2e_cross_signing_uia_bypass (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    updatable_before_ts BIGINT NOT NULL
);
