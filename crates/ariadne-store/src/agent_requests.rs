//! Persistent questions an agent has explicitly addressed to a person.

use ariadne_core::id::new_id;

use crate::{Result, Store, now};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentRequest {
    pub id: String,
    pub session_id: String,
    pub summary: String,
    pub created_at: String,
    pub answered_at: Option<String>,
    pub last_raised_turn_id: Option<String>,
    /// The JSON of the whole question, where the agent asked one.
    pub question: Option<String>,
    /// `answered`, `withdrawn` or `expired` once the request has closed;
    /// `claimed` while a claimed answer is on its way to the agent, and
    /// `claimed_expired` where a turn ended meanwhile.
    pub resolution: Option<String>,
    /// The JSON of the recorded answer, where the user answered.
    pub answer: Option<String>,
}

/// How a claimed request ended once its answer was sent or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimSettlement {
    /// The answer reached the agent.
    Answered,
    /// The answer never reached the agent, and the request waits again.
    Reopened,
    /// The answer never reached the agent, and a turn that ended meanwhile
    /// expired the request.
    Expired,
}

impl Store {
    /// Open one request, or return the pending request this session
    /// already holds under the same summary. The flag says which: `true`
    /// for a row this call created, `false` for the one it found.
    ///
    /// A repeat keeps the request and its question. A repeat with no
    /// question raises it again as it is; a repeat whose question differs
    /// from the one the request holds, or adds one where it holds none, is
    /// a conflict, and changes nothing: a card already drawn is never
    /// answered with options it does not show.
    pub async fn create_agent_request(
        &self,
        session_id: &str,
        summary: &str,
        question: Option<&str>,
        turn_id: Option<&str>,
    ) -> Result<(AgentRequest, bool)> {
        // A pending row that closes between the insert and the repeat is
        // gone: the next round opens a fresh one in its place.
        for _ in 0..3 {
            let id = new_id();
            let result = sqlx::query(
                "INSERT INTO agent_requests (id, session_id, summary, created_at, last_raised_turn_id, question)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(session_id)
            .bind(summary)
            .bind(now())
            .bind(turn_id)
            .bind(question)
            .execute(self.w())
            .await;
            match result {
                Ok(_) => {
                    let request = self.get_agent_request(&id).await?;
                    self.publish_session_update(session_id).await?;
                    return Ok((request, true));
                }
                Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {}
                Err(error) => return Err(error.into()),
            }
            let repeated: Option<AgentRequest> = sqlx::query_as(
                "UPDATE agent_requests SET last_raised_turn_id = ?
                 WHERE session_id = ? AND summary = ? AND answered_at IS NULL
                       AND (? IS NULL OR question IS ?)
                 RETURNING *",
            )
            .bind(turn_id)
            .bind(session_id)
            .bind(summary)
            .bind(question)
            .bind(question)
            .fetch_optional(self.w())
            .await?;
            if let Some(existing) = repeated {
                self.publish_session_update(session_id).await?;
                return Ok((existing, false));
            }
            let pending: Option<String> = sqlx::query_scalar(
                "SELECT id FROM agent_requests
                 WHERE session_id = ? AND summary = ? AND answered_at IS NULL",
            )
            .bind(session_id)
            .bind(summary)
            .fetch_optional(self.r())
            .await?;
            if let Some(pending) = pending {
                return Err(crate::StoreError::Conflict(format!(
                    "request {pending} already waits under this hint with another question: \
                     withdraw it first, or give the new question another hint"
                )));
            }
        }
        Err(crate::StoreError::Conflict(
            "the request under this hint kept closing: call `wait_for_user` again".into(),
        ))
    }

    pub async fn pending_agent_requests(&self) -> Result<Vec<AgentRequest>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_requests WHERE answered_at IS NULL ORDER BY id")
                .fetch_all(self.r())
                .await?,
        )
    }

    /// The requests a console input will answer before it reaches the agent,
    /// oldest first.
    pub async fn pending_session_agent_requests(
        &self,
        session_id: &str,
    ) -> Result<Vec<AgentRequest>> {
        Ok(sqlx::query_as(
            "SELECT * FROM agent_requests
             WHERE session_id = ? AND answered_at IS NULL ORDER BY id",
        )
        .bind(session_id)
        .fetch_all(self.r())
        .await?)
    }

    /// End one turn's wait. A pending request survives only where that turn
    /// called `wait_for_user` with its hint, including a repeated hint. The
    /// answer is the ids of the requests it closed as `expired`.
    pub async fn finish_agent_request_turn(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Result<Vec<String>> {
        // One statement, so no claim settles between its two halves: a
        // pending request expires, and a claimed answer still on its way to
        // the agent is marked, the claimant's to settle — it expires only if
        // that delivery fails.
        let closed: Vec<(String, String)> = sqlx::query_as(
            "UPDATE agent_requests
             SET answered_at = COALESCE(answered_at, ?),
                 resolution = CASE WHEN resolution = 'claimed' THEN 'claimed_expired'
                                   ELSE 'expired' END
             WHERE session_id = ?
                   AND (answered_at IS NULL OR resolution = 'claimed')
                   AND (last_raised_turn_id IS NULL OR last_raised_turn_id <> ?)
             RETURNING id, resolution",
        )
        .bind(now())
        .bind(session_id)
        .bind(turn_id)
        .fetch_all(self.w())
        .await?;
        let expired: Vec<String> = closed
            .into_iter()
            .filter(|(_, resolution)| resolution == "expired")
            .map(|(id, _)| id)
            .collect();
        if !expired.is_empty() {
            self.publish_session_update(session_id).await?;
        }
        Ok(expired)
    }

    /// Claim one pending request for `answer`, before the answer is sent:
    /// the row stops being pending, so no other answer, withdrawal or turn
    /// end closes it meanwhile. `None` where the request is no longer
    /// pending: answered, withdrawn, expired, or claimed already. The
    /// claimant settles it with [`Store::settle_agent_request_claim`].
    pub async fn claim_agent_request(
        &self,
        id: &str,
        session_id: &str,
        answer: &str,
    ) -> Result<Option<AgentRequest>> {
        let claimed: Option<AgentRequest> = sqlx::query_as(
            "UPDATE agent_requests SET answered_at = ?, resolution = 'claimed', answer = ?
             WHERE id = ? AND session_id = ? AND answered_at IS NULL
             RETURNING *",
        )
        .bind(now())
        .bind(answer)
        .bind(id)
        .bind(session_id)
        .fetch_optional(self.w())
        .await?;
        if claimed.is_some() {
            self.publish_session_update(session_id).await?;
        }
        Ok(claimed)
    }

    /// Settle a claim once its answer was sent, or failed to be sent.
    ///
    /// A delivered answer closes the request as answered. An undelivered
    /// one opens it again, unless a turn ended meanwhile that would have
    /// expired it, or a newer request took its hint: then it expires.
    pub async fn settle_agent_request_claim(
        &self,
        claimed: &AgentRequest,
        delivered: bool,
    ) -> Result<ClaimSettlement> {
        let session_id = claimed.session_id.as_str();
        let settled = if delivered {
            sqlx::query(
                "UPDATE agent_requests SET resolution = 'answered'
                 WHERE id = ? AND resolution IN ('claimed', 'claimed_expired')",
            )
            .bind(&claimed.id)
            .execute(self.w())
            .await?;
            ClaimSettlement::Answered
        } else {
            let reopened = sqlx::query(
                "UPDATE agent_requests SET answered_at = NULL, resolution = NULL, answer = NULL
                 WHERE id = ? AND resolution = 'claimed' AND answered_at = ?",
            )
            .bind(&claimed.id)
            .bind(&claimed.answered_at)
            .execute(self.w())
            .await;
            let reopened = match reopened {
                Ok(result) => result.rows_affected() > 0,
                Err(sqlx::Error::Database(error)) if error.is_unique_violation() => false,
                Err(error) => return Err(error.into()),
            };
            if reopened {
                ClaimSettlement::Reopened
            } else {
                sqlx::query(
                    "UPDATE agent_requests SET resolution = 'expired', answer = NULL
                     WHERE id = ? AND resolution IN ('claimed', 'claimed_expired')",
                )
                .bind(&claimed.id)
                .execute(self.w())
                .await?;
                ClaimSettlement::Expired
            }
        };
        self.publish_session_update(session_id).await?;
        Ok(settled)
    }

    /// Withdraw one pending request. True where this call closed it.
    pub async fn withdraw_agent_request(&self, id: &str, session_id: &str) -> Result<bool> {
        let withdrawn = sqlx::query(
            "UPDATE agent_requests SET answered_at = ?, resolution = 'withdrawn'
             WHERE id = ? AND session_id = ? AND answered_at IS NULL",
        )
        .bind(now())
        .bind(id)
        .bind(session_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_session_update(session_id).await?;
        Ok(withdrawn > 0)
    }

    /// One request of one session, pending or closed.
    pub async fn session_agent_request(
        &self,
        id: &str,
        session_id: &str,
    ) -> Result<Option<AgentRequest>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_requests WHERE id = ? AND session_id = ?")
                .bind(id)
                .bind(session_id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    async fn get_agent_request(&self, id: &str) -> Result<AgentRequest> {
        Ok(sqlx::query_as("SELECT * FROM agent_requests WHERE id = ?")
            .bind(id)
            .fetch_one(self.r())
            .await?)
    }
}
