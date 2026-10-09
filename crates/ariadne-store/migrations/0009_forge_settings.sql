-- The one forge settings row (027): the tunnel switch, and the subdomain the
-- tunnel asks for, so its public URL survives a restart where the server
-- grants it again.
CREATE TABLE forge_settings (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    tunnel_enabled   INTEGER NOT NULL DEFAULT 1 CHECK (tunnel_enabled IN (0, 1)),
    -- NULL until the first tunnel picks one.
    tunnel_subdomain TEXT,
    updated_at       TEXT NOT NULL
);
INSERT INTO forge_settings (id, updated_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
