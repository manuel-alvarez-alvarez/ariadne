-- The webhook tunnel moved from localtunnel to smee.io (027). A smee channel
-- is public: anyone who knows its URL reads every delivery sent to it, so a
-- channel must be a long random name, never a guessable localtunnel
-- subdomain. Rename the column and forget the old subdomain; the next tunnel
-- picks a fresh channel and keeps it.
ALTER TABLE forge_settings RENAME COLUMN tunnel_subdomain TO tunnel_channel;
UPDATE forge_settings SET tunnel_channel = NULL;
