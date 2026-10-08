-- The one summary comment an Ariadne review keeps on a request (029): its
-- forge id, written when the review first posts it, and edited in place on
-- every later round. NULL until the first review of the request.
ALTER TABLE pull_requests ADD COLUMN summary_comment_id TEXT;
