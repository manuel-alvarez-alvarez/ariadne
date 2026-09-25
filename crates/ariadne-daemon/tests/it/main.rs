//! The daemon's integration tests, as one test binary.
//!
//! Each module was a binary of its own once, and every one of them compiled
//! `common` again and linked the whole daemon again: a change to the daemon
//! rebuilt and relinked 31 binaries. One binary compiles `common` once and
//! links once. nextest still runs each test in a process of its own, so no
//! test shares state with another.
//!
//! A new file under this directory is a test only when it is named here.

mod common;

mod acp_console;
mod acp_discovery;
mod acp_runtime;
mod acp_session_resume;
mod acp_terminal;
mod adapters;
mod agent_messages;
mod agents;
mod doctor;
mod events;
mod goal_completion;
mod goal_delete;
mod goal_repositories;
mod landing_lifecycle;
mod logs;
mod managers;
mod models;
mod multi_author_tasks;
mod pins;
mod plan_finalize;
mod prompts;
mod repositories;
mod resume;
mod scheduler_attention;
mod scheduler_dependencies;
mod session_list;
mod session_start;
mod skill_documents;
mod stored_conversations;
mod stored_conversations_opencode;
mod task_branches;
mod task_failure;
mod transcript_usage;
mod unknown_fields;
mod unreviewed_tasks;
