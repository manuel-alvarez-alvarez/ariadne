//! Searchable facts learned about one repository, or about every repository.

use ariadne_core::id::new_id;

use crate::{Change, Memory, Result, Store, now};

#[derive(Debug, Clone)]
pub struct NewMemory {
    /// The repository the fact is about. `None` saves a global fact.
    pub repository_id: Option<String>,
    pub text: String,
    /// The session that taught the fact, and its task and goal. All `None`
    /// when the user wrote it.
    pub source_session_id: Option<String>,
    pub source_task_id: Option<String>,
    pub source_goal_id: Option<String>,
    /// When the fact stops being read. `None` never expires.
    pub expires_at: Option<String>,
}

/// Which memories a list or a search reads.
#[derive(Debug, Clone)]
pub enum MemoryScope {
    /// Every memory, of every repository and of none.
    All,
    /// The memories of no repository.
    Global,
    /// The memories of these repositories, and the global ones too when
    /// `global` is set.
    Repositories { ids: Vec<String>, global: bool },
}

impl MemoryScope {
    /// The condition that holds this scope, with one `?` per bound id.
    fn clause(&self) -> String {
        match self {
            Self::All => "1 = 1".into(),
            Self::Global => "repository_id IS NULL".into(),
            Self::Repositories { ids, global } => {
                let repositories = if ids.is_empty() {
                    "0 = 1".to_string()
                } else {
                    let marks = vec!["?"; ids.len()].join(", ");
                    format!("repository_id IN ({marks})")
                };
                if *global {
                    format!("({repositories} OR repository_id IS NULL)")
                } else {
                    repositories
                }
            }
        }
    }

    /// The repository ids the clause binds, in order.
    fn ids(&self) -> &[String] {
        match self {
            Self::All | Self::Global => &[],
            Self::Repositories { ids, .. } => ids,
        }
    }
}

impl Store {
    pub async fn create_memory(&self, new: NewMemory) -> Result<Memory> {
        self.create_memory_checked(new, None).await
    }

    /// Save a sourced memory only while its task, or taskless goal, is below
    /// `source_limit`. The duplicate check, count and insert share the writer
    /// transaction.
    pub async fn create_memory_with_source_limit(
        &self,
        new: NewMemory,
        source_limit: i64,
    ) -> Result<Memory> {
        self.create_memory_checked(new, Some(source_limit)).await
    }

    async fn create_memory_checked(
        &self,
        new: NewMemory,
        source_limit: Option<i64>,
    ) -> Result<Memory> {
        if let Some(repository_id) = &new.repository_id {
            self.get_repository(repository_id).await?;
        }
        let mut transaction = self.w().begin().await?;
        if let Some(fts_query) = fts_query(&new.text) {
            let scope = if new.repository_id.is_some() {
                "memories.repository_id = ?"
            } else {
                "memories.repository_id IS NULL"
            };
            let sql = format!(
                "SELECT memories.* FROM memories_fts
                  JOIN memories ON memories.rowid = memories_fts.rowid
                  WHERE memories_fts MATCH ? AND {scope}
                    AND (expires_at IS NULL OR expires_at > ?)
                  ORDER BY bm25(memories_fts), memories.id DESC"
            );
            let mut query = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql)).bind(fts_query);
            if let Some(repository_id) = &new.repository_id {
                query = query.bind(repository_id);
            }
            if let Some(memory) = query.bind(now()).fetch_optional(&mut *transaction).await? {
                return Err(crate::StoreError::Conflict(format!(
                    "Memory {} already holds this fact: {}. Do not save it. Use that memory.",
                    memory.id, memory.text
                )));
            }
        }
        if let Some(source_limit) = source_limit {
            let count: i64 = match new.source_task_id.as_deref() {
                Some(task_id) => {
                    sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE source_task_id = ?")
                        .bind(task_id)
                        .fetch_one(&mut *transaction)
                        .await?
                }
                None => {
                    sqlx::query_scalar(
                        "SELECT COUNT(*) FROM memories
                      WHERE source_task_id IS NULL AND source_goal_id = ?",
                    )
                    .bind(&new.source_goal_id)
                    .fetch_one(&mut *transaction)
                    .await?
                }
            };
            if count >= source_limit {
                return Err(crate::StoreError::Conflict(
                    "memory source limit reached".into(),
                ));
            }
        }
        let id = new_id();
        sqlx::query(
            "INSERT INTO memories
                (id, repository_id, text, source_session_id, source_task_id,
                 source_goal_id, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(&new.text)
        .bind(&new.source_session_id)
        .bind(&new.source_task_id)
        .bind(&new.source_goal_id)
        .bind(now())
        .bind(&new.expires_at)
        .execute(&mut *transaction)
        .await?;
        let memory = sqlx::query_as::<_, Memory>("SELECT * FROM memories WHERE id = ?")
            .bind(&id)
            .fetch_one(&mut *transaction)
            .await?;
        transaction.commit().await?;
        self.publish(Change::MemoryCreated(memory.clone()));
        Ok(memory)
    }

    pub async fn get_memory(&self, id: &str) -> Result<Memory> {
        self.fetch_by("memory", "memories", "id", id).await
    }

    /// List the active memories of a scope, newest first.
    pub async fn list_memories(&self, scope: &MemoryScope) -> Result<Vec<Memory>> {
        self.active_memories(scope).await
    }

    /// Find the active memories of a scope that match `query`, best first.
    pub async fn search_memories(&self, scope: &MemoryScope, query: &str) -> Result<Vec<Memory>> {
        self.matching_memories(scope, query).await
    }

    /// The active memories that match a word of `query`, best first.
    pub async fn matching_memories(&self, scope: &MemoryScope, query: &str) -> Result<Vec<Memory>> {
        let Some(fts_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT memories.* FROM memories_fts
              JOIN memories ON memories.rowid = memories_fts.rowid
              WHERE memories_fts MATCH ? AND {} \
                AND (expires_at IS NULL OR expires_at > ?)
              ORDER BY bm25(memories_fts), memories.id DESC",
            scope.clause()
        );
        let mut query = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql));
        query = query.bind(fts_query);
        for id in scope.ids() {
            query = query.bind(id.as_str());
        }
        query = query.bind(now());
        Ok(query.fetch_all(self.r()).await?)
    }

    /// The memories of a scope that have not expired, newest first.
    ///
    /// Only fixed fragments are assembled — the ids and the text go in as
    /// bindings — which is what makes the statement safe to assert.
    async fn active_memories(&self, scope: &MemoryScope) -> Result<Vec<Memory>> {
        let sql = format!(
            "SELECT * FROM memories
              WHERE {} AND (expires_at IS NULL OR expires_at > ?)",
            scope.clause()
        );
        let sql = format!("{sql} ORDER BY id DESC");
        let mut query = sqlx::query_as::<_, Memory>(sqlx::AssertSqlSafe(sql));
        for id in scope.ids() {
            query = query.bind(id.as_str());
        }
        query = query.bind(now());
        Ok(query.fetch_all(self.r()).await?)
    }

    pub async fn delete_memory(&self, id: &str) -> Result<()> {
        let memory = self.get_memory(id).await?;
        sqlx::query("DELETE FROM memories WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        self.publish(Change::MemoryDeleted {
            id: memory.id,
            repository_id: memory.repository_id,
        });
        Ok(())
    }
}

/// The FTS5 expression for the words a caller typed. Each word is a prefix,
/// and any word may match. `None` means the query has no word.
fn fts_query(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    (!words.is_empty()).then(|| {
        words
            .iter()
            .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}
