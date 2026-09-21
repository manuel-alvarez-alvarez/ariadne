//! The knowledge base in the daemon: what is indexed, and when.
//!
//! One worker reads one repository at a time, off the request path, and the
//! daemon's own event stream is what tells it when: a repository registered
//! or edited is read at its base branch, a task branch that moved (002, rule
//! 11) is read at its new head, and a landing reads the base branch again.
//! At start every base branch and every in-flight task branch is queued.
//!
//! What a run did is published on the bus as `knowledge_indexed`, and a
//! failed run as `knowledge_failed`, straight onto the bus like a branch
//! move: nothing in the daemon's database changed. A run is of one ref, and
//! so is its outcome: the state and the error are kept per ref, and a good
//! run of a task branch leaves the failure of the base branch in place.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use ariadne_api::knowledge::{KnowledgeFailedDto, KnowledgeIndexedDto};
use ariadne_api::stream::DomainEvent;
use ariadne_core::TaskStatus;
use ariadne_knowledge::{KnowledgeStore, State};
use ariadne_store::{Store, TaskFilter};

use crate::bus::{BusEvent, EventBus};

/// The knowledge base as the handlers see it: a store to ask, and a queue
/// to put a repository on. Disabled by `knowledge_enabled = false`, it holds
/// neither.
#[derive(Clone)]
pub struct Knowledge {
    inner: Option<Arc<Enabled>>,
}

struct Enabled {
    store: KnowledgeStore,
    jobs: mpsc::UnboundedSender<Job>,
}

/// One unit of work for the worker. Each names a repository by id and reads
/// its path when it runs, so a repository deleted while queued is skipped.
#[derive(Debug)]
enum Job {
    /// Read one ref of one repository from where it was last read. A run
    /// that fails marks the ref failed; a lenient one is for a task branch
    /// that may be gone, and drops the ref's rows where git cannot resolve
    /// it.
    Index {
        repository_id: String,
        git_ref: String,
        strict: bool,
    },
    /// Drop a repository's rows and read every ref it had again.
    Reindex { repository_id: String },
    /// Forget a repository.
    Drop { repository_id: String },
    /// Forget a task's branches: the branch itself and the author branches
    /// beside it, once the task has landed.
    DropTaskBranches {
        repository_id: String,
        branch: String,
    },
}

impl Knowledge {
    /// A knowledge base that indexes nothing and serves nothing.
    pub(crate) fn disabled() -> Self {
        Self { inner: None }
    }

    /// Open the store at `db_path`, start the worker and the follower, and
    /// queue every base branch and in-flight task branch. `enabled = false`
    /// is [`Self::disabled`]. An index run parses `workers` files at a time.
    pub async fn start(
        enabled: bool,
        workers: usize,
        db_path: PathBuf,
        store: Store,
        events: EventBus,
    ) -> Result<Self> {
        if !enabled {
            return Ok(Self::disabled());
        }
        let knowledge = KnowledgeStore::open(&db_path)
            .await
            .with_context(|| format!("opening the knowledge store {}", db_path.display()))?
            .with_workers(workers);
        let (jobs, rx) = mpsc::unbounded_channel();
        tokio::spawn(worker(knowledge.clone(), store.clone(), events.clone(), rx));
        tokio::spawn(follow(store.clone(), events.subscribe(), jobs.clone()));
        queue_every_branch(&store, &jobs).await?;
        Ok(Self {
            inner: Some(Arc::new(Enabled {
                store: knowledge,
                jobs,
            })),
        })
    }

    /// The store, where the knowledge base is enabled.
    pub fn store(&self) -> Option<&KnowledgeStore> {
        self.inner.as_ref().map(|inner| &inner.store)
    }

    /// Queue one ref of a repository.
    pub fn index(&self, repository_id: &str, git_ref: &str) {
        if let Some(inner) = &self.inner {
            let _ = inner.jobs.send(Job::Index {
                repository_id: repository_id.to_string(),
                git_ref: git_ref.to_string(),
                strict: true,
            });
        }
    }

    /// Drop a repository's rows and index it again. Its base branch, and so
    /// the repository, reads as `indexing` from here until the worker is
    /// done with it.
    pub(crate) async fn reindex(&self, repository_id: &str, base_branch: &str) -> Result<()> {
        let Some(inner) = &self.inner else {
            return Ok(());
        };
        inner
            .store
            .set_ref_state(repository_id, base_branch, State::Indexing, None)
            .await?;
        let _ = inner.jobs.send(Job::Reindex {
            repository_id: repository_id.to_string(),
        });
        Ok(())
    }
}

/// Queue every base branch and every in-flight task branch: what a start
/// reads, and what a follower that fell behind the event stream reads again.
async fn queue_every_branch(store: &Store, jobs: &mpsc::UnboundedSender<Job>) -> Result<()> {
    for repository in store.list_repositories().await? {
        let _ = jobs.send(Job::Index {
            repository_id: repository.id,
            git_ref: repository.base_branch,
            strict: true,
        });
    }
    // Leniently: a branch a user deleted by hand while the daemon was
    // down is no failure of the repository.
    for task in store.list_tasks(TaskFilter::default()).await? {
        if !task.status().is_terminal() && task.worktree_path.is_some() {
            let _ = jobs.send(Job::Index {
                repository_id: task.repo_id,
                git_ref: task.branch,
                strict: false,
            });
        }
    }
    Ok(())
}

/// Turn the daemon's events into jobs, until the bus closes.
async fn follow(
    store: Store,
    mut rx: broadcast::Receiver<BusEvent>,
    jobs: mpsc::UnboundedSender<Job>,
) {
    loop {
        let event = match rx.recv().await {
            Ok(event) => event.event,
            // The missed events are gone, and any of them may have been a
            // branch that moved. A run of a ref that did not move reads
            // nothing, so every branch is queued again.
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                warn!(
                    missed,
                    "the knowledge base fell behind the event stream: reading every branch again"
                );
                if let Err(e) = queue_every_branch(&store, &jobs).await {
                    warn!(error = %e, "cannot queue the branches again");
                }
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        };
        let queued = match event {
            DomainEvent::RepositoryCreated(repository)
            | DomainEvent::RepositoryUpdated(repository) => vec![Job::Index {
                repository_id: repository.id,
                git_ref: repository.base_branch,
                strict: true,
            }],
            DomainEvent::RepositoryDeleted(deleted) => vec![Job::Drop {
                repository_id: deleted.id,
            }],
            DomainEvent::TaskBranchUpdated(moved) => match store.get_task(&moved.task_id).await {
                Ok(task) => vec![Job::Index {
                    repository_id: task.repo_id,
                    git_ref: moved.branch,
                    strict: true,
                }],
                Err(e) => {
                    debug!(task = %moved.task_id, error = %e, "a branch moved on a task that is gone");
                    continue;
                }
            },
            // A landing: the base branch has the task's commit now, and the
            // task's branches have nothing the base branch lacks.
            DomainEvent::TaskUpdated(updated)
                if updated.transition.is_some() && updated.task.status == TaskStatus::Finished =>
            {
                match store.get_repository(&updated.task.repo_id).await {
                    Ok(repository) => vec![
                        Job::DropTaskBranches {
                            repository_id: repository.id.clone(),
                            branch: updated.task.branch,
                        },
                        Job::Index {
                            repository_id: repository.id,
                            git_ref: repository.base_branch,
                            strict: true,
                        },
                    ],
                    Err(_) => continue,
                }
            }
            _ => continue,
        };
        for job in queued {
            if jobs.send(job).is_err() {
                return;
            }
        }
    }
}

/// Run the jobs, one at a time, in the order they came.
async fn worker(
    knowledge: KnowledgeStore,
    store: Store,
    events: EventBus,
    mut rx: mpsc::UnboundedReceiver<Job>,
) {
    while let Some(job) = rx.recv().await {
        match job {
            Job::Index {
                repository_id,
                git_ref,
                strict,
            } => {
                run(
                    &knowledge,
                    &store,
                    &events,
                    &repository_id,
                    &git_ref,
                    strict,
                )
                .await;
            }
            Job::Reindex { repository_id } => {
                let Ok(repository) = store.get_repository(&repository_id).await else {
                    continue;
                };
                let mut refs = knowledge
                    .ref_names(&repository_id)
                    .await
                    .unwrap_or_default();
                if let Err(e) = knowledge.drop_repository(&repository_id).await {
                    warn!(repository = %repository_id, error = %e, "cannot drop the repository's index");
                }
                refs.retain(|git_ref| *git_ref != repository.base_branch);
                // The states went with the rows. Each ref reads as in its
                // first index from here, not as never indexed.
                for git_ref in std::iter::once(&repository.base_branch).chain(&refs) {
                    if let Err(e) = knowledge
                        .set_ref_state(&repository_id, git_ref, State::Indexing, None)
                        .await
                    {
                        warn!(repository = %repository_id, git_ref, error = %e, "cannot record the indexing state");
                    }
                }
                run(
                    &knowledge,
                    &store,
                    &events,
                    &repository_id,
                    &repository.base_branch,
                    true,
                )
                .await;
                // A task branch the repository no longer has is not a failure
                // of the reindex: it is simply not read again.
                for git_ref in refs {
                    run(&knowledge, &store, &events, &repository_id, &git_ref, false).await;
                }
            }
            Job::Drop { repository_id } => {
                if let Err(e) = knowledge.drop_repository(&repository_id).await {
                    warn!(repository = %repository_id, error = %e, "cannot drop the repository's index");
                }
            }
            Job::DropTaskBranches {
                repository_id,
                branch,
            } => {
                // The task's own branch, and the `-a<n>` branches of a task
                // staffed with several authors (002, rule 6).
                let authors = format!("{branch}-a");
                let refs = knowledge
                    .ref_names(&repository_id)
                    .await
                    .unwrap_or_default();
                for git_ref in refs
                    .into_iter()
                    .filter(|git_ref| *git_ref == branch || git_ref.starts_with(&authors))
                {
                    debug!(repository = %repository_id, git_ref, "dropping a landed task branch");
                    if let Err(e) = knowledge.drop_ref(&repository_id, &git_ref).await {
                        warn!(repository = %repository_id, git_ref, error = %e, "cannot drop the branch's index");
                    }
                }
            }
        }
    }
}

/// Read one ref, and say how it went, on that ref alone. A lenient run is
/// for a task branch that may be gone: where git cannot resolve the branch,
/// its rows go and nothing failed. Every other error of a lenient run is a
/// failure like that of a strict one, and the rows stay.
async fn run(
    knowledge: &KnowledgeStore,
    store: &Store,
    events: &EventBus,
    repository_id: &str,
    git_ref: &str,
    strict: bool,
) {
    let Ok(repository) = store.get_repository(repository_id).await else {
        debug!(repository = %repository_id, "not indexing a repository that is gone");
        return;
    };
    if let Err(e) = knowledge
        .set_ref_state(repository_id, git_ref, State::Indexing, None)
        .await
    {
        warn!(repository = %repository_id, error = %e, "cannot record the indexing state");
    }
    // The base branch is what another repository is linked against.
    if git_ref == repository.base_branch
        && let Err(e) = knowledge.set_base_ref(repository_id, git_ref).await
    {
        warn!(repository = %repository_id, error = %e, "cannot record the base branch");
    }
    match knowledge
        .index(repository_id, Path::new(&repository.path), git_ref)
        .await
    {
        Ok(indexed) => {
            info!(
                repository = %repository_id, git_ref, commit = %indexed.commit,
                files = indexed.files, symbols = indexed.symbols, parsed = indexed.parsed,
                "indexed"
            );
            let _ = knowledge
                .set_ref_state(repository_id, git_ref, State::Idle, None)
                .await;
            publish(
                events,
                DomainEvent::KnowledgeIndexed(KnowledgeIndexedDto {
                    repository_id: repository_id.to_string(),
                    git_ref: git_ref.to_string(),
                    commit: indexed.commit,
                    files: indexed.files,
                    symbols: indexed.symbols,
                }),
            );
        }
        Err(e) => {
            if !strict && ref_is_gone(Path::new(&repository.path), git_ref).await {
                debug!(repository = %repository_id, git_ref, error = %e, "a branch is gone: dropping it");
                if let Err(e) = knowledge.drop_ref(repository_id, git_ref).await {
                    warn!(repository = %repository_id, git_ref, error = %e, "cannot drop the ref's index");
                }
                return;
            }
            let error = format!("{e:#}");
            warn!(repository = %repository_id, git_ref, error = %error, "indexing failed");
            let _ = knowledge
                .set_ref_state(repository_id, git_ref, State::Failed, Some(&error))
                .await;
            publish(
                events,
                DomainEvent::KnowledgeFailed(KnowledgeFailedDto {
                    repository_id: repository_id.to_string(),
                    git_ref: git_ref.to_string(),
                    error,
                }),
            );
        }
    }
}

/// Whether git says that `git_ref` names no commit of `repo`. `rev-parse
/// --verify --quiet` exits with 1 for that alone: a repository git cannot
/// read exits with 128, and a git that did not start exits with nothing. Only
/// the first is a branch that is gone.
pub(crate) async fn ref_is_gone(repo: &Path, git_ref: &str) -> bool {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{git_ref}^{{commit}}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    matches!(status, Ok(status) if status.code() == Some(1))
}

/// Knowledge events belong to no goal and no task.
fn publish(events: &EventBus, event: DomainEvent) {
    events.publish(BusEvent {
        event,
        goal_id: None,
        task_id: None,
        recorded: None,
    });
}
