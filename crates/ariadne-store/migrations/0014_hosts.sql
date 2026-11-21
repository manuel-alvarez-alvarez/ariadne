CREATE TABLE hosts (
    name TEXT PRIMARY KEY NOT NULL,
    endpoint TEXT,
    remote TEXT,
    color TEXT CHECK (color IN ('red','orange','amber','green','teal','blue','violet','pink')),
    CHECK ((name = 'local' AND endpoint IS NULL AND remote IS NULL) OR (name != 'local' AND endpoint IS NOT NULL))
);
