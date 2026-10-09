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
mod ai_permissions;
mod ai_permissions_decisions;
mod ai_permissions_flavours;
mod ai_permissions_server;
mod auto_switch;
mod doctor;
mod events;
mod failure_diagnosis;
mod final_tasks;
mod forge_integration;
mod goal_completion;
mod goal_delete;
mod goal_repositories;
mod issues;
mod landing_lifecycle;
mod learned_permissions;
mod logs;
mod managers;
mod models;
mod multi_author_tasks;
mod outcome_stats;
mod pins;
mod plan_finalize;
mod prompts;
mod repositories;
mod resume;
mod review_stats;
mod scheduler_attention;
mod scheduler_dependencies;
mod session_list;
mod session_start;
mod skill_documents;
mod stats;
mod stats_attention;
mod stats_models;
mod stats_spend;
mod stats_time;
mod stats_work;
mod stored_conversations;
mod stored_conversations_opencode;
mod switch;
mod switch_stats;
mod task_branches;
mod task_failure;
mod transcript_usage;
mod unknown_fields;
mod unreviewed_tasks;
mod workflows;

mod kept_requests;
mod pull_request_reviews;
mod pull_requests;

mod tunnel;
mod webhooks;

mod workflow_steps;
