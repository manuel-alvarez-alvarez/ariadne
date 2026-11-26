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
mod acp_terminal;
mod adapters;
mod agents;
mod ai_permissions;
mod ai_permissions_decisions;
mod ai_permissions_flavours;
mod ai_permissions_server;
mod attention;
mod auto_switch;
mod dashboards;
mod docs;
mod doctor;
mod events;
mod failure_diagnosis;
mod far_tasks;
mod forge_host_limit;
mod forge_integration;
mod forge_rate_limit;
mod goal_completion;
mod goal_delete;
mod goal_repositories;
mod home_skills;
mod host_health;
mod host_queries;
mod host_reconnect;
mod host_sync;
mod hosts;
mod issues;
mod learned_permissions;
mod linked_goals;
mod logs;
mod managers;
mod metrics;
mod models;
mod pins;
mod plan_finalize;
mod plan_mode;
mod portable_config;
mod posts;
mod prompts;
mod relay;
mod remote_tasks;
mod repositories;
mod resume;
mod scheduler_attention;
mod scheduler_dependencies;
mod seat_brief;
mod session_list;
mod shutdown;
mod skill_documents;
mod ssh_forward;
mod stub_lifetime;
mod subagents;
mod switch;
mod task_branches;
mod task_failure;
mod telemetry_query;
mod trajectory;
mod transcript_usage;
mod unknown_fields;
mod workflows;

mod kept_requests;
mod pull_request_babysit;
mod pull_request_detail_skip;
mod pull_request_keep_retry;
mod pull_request_lists;
mod pull_request_reviews;
mod pull_requests;
mod questions;

mod tunnel;
mod webhooks;

mod workflow_pull_request;
mod workflow_steps;

mod place;

mod acp_recovery;

mod standalone_tasks;
