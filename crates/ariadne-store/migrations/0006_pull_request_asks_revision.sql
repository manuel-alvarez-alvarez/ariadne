-- Who wants a request moves by a counter (026): every write of a review ask
-- (029), a keeper ask (032) or a task adopting the request adds one, so a
-- pass that stops working on a request can tell an ask that landed since its
-- read, even one that leaves the asks as they were.
ALTER TABLE pull_requests ADD COLUMN asks_revision INTEGER NOT NULL DEFAULT 0;
