DROP TABLE IF EXISTS event_phases;
CREATE TABLE IF NOT EXISTS event_phases
(
    event_id text NOT NULL PRIMARY KEY,
    curr text NOT NULL,
    next text NOT NULL,
    goal text NOT NULL default 'timline'
);
CREATE INDEX IF NOT EXISTS idx_event_phases_curr ON event_phases (curr);
CREATE INDEX IF NOT EXISTS idx_event_phases_next ON event_phases (next);

DROP TABLE IF EXISTS event_missings;
CREATE TABLE IF NOT EXISTS event_missings
(
    id serial NOT NULL PRIMARY KEY,
    room_id text NOT NULL,
    event_id text NOT NULL,
    event_sn bigint NOT NULL,
    missing_id text NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_event_missings_event_id ON event_missings (event_id);
CREATE INDEX IF NOT EXISTS idx_event_missings_event_sn ON event_missings (event_sn);
CREATE INDEX IF NOT EXISTS idx_event_missings_missing_id ON event_missings (missing_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_event_missings_event_id_missing_id ON event_missings (event_id, missing_id);

DROP TABLE IF EXISTS timeline_gaps;