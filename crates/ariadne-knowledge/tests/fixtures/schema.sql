-- The fixture schema.
CREATE TABLE users (id INT, name TEXT);

ALTER TABLE users ADD COLUMN email TEXT;

CREATE VIEW active_users AS SELECT id FROM users;

ALTER VIEW active_users RENAME TO current_users;

CREATE INDEX users_id ON users (id);

ALTER INDEX users_id RENAME TO users_id_idx;

CREATE SEQUENCE user_ids;

CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');

CREATE SCHEMA app;

CREATE MATERIALIZED VIEW recent_users AS SELECT id FROM users;

CREATE FUNCTION next_id() RETURNS INT AS $$ SELECT 1 $$ LANGUAGE SQL;

CREATE TRIGGER users_trigger BEFORE INSERT ON users FOR EACH ROW EXECUTE PROCEDURE next_id();
