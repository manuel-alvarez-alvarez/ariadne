CREATE TABLE session (
    id TEXT PRIMARY KEY,
    directory TEXT NOT NULL,
    time_updated INTEGER NOT NULL
);
CREATE TABLE message (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    data TEXT NOT NULL
);
CREATE TABLE part (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    time_created INTEGER NOT NULL,
    data TEXT NOT NULL
);
INSERT INTO session VALUES ('opencode-outside', '/work/opencode', 1788257100000);
INSERT INTO message VALUES ('message-outside', 'opencode-outside', 1788256800000, '{"role":"user"}');
INSERT INTO part VALUES ('part-outside', 'message-outside', 1788256800001, '{"type":"text","text":"Write the integration test."}');
