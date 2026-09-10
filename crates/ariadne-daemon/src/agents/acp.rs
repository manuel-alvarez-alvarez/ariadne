//! Agent Client Protocol adapter.
//!
//! - Binary: `acp`, an ACP agent executable selected by the user on `PATH`
//! - Transport: stable ACP v1 over newline-delimited JSON-RPC on stdio
//! - Model and effort: session configuration options with the `model` and
//!   `thought_level` categories
//! - System prompt and skills: prepended to the first user message
//! - MCP: the stdio server in `session/new`, `session/resume`, or
//!   `session/load`
//! - Events: the CLI-side ACP client maps updates to the existing hook event
//!   vocabulary and invokes `ariadne agent-event --kind acp`
//! - Session id: returned by `session/new` and captured from `session_start`
//! - Resume: `session/resume`, with `session/load` as the protocol fallback
//! - Compaction: a completed `compaction_update`

use anyhow::{Context, Result};
use ariadne_core::AgentKind;
use ariadne_core::acp::{self, EnvVariable, Hook, LaunchConfig, McpServer};

use super::contract::{AdapterContract, EventDelivery, InstructionDelivery, Spelling};
use super::{AgentAdapter, SpawnCtx, SpawnPlan, base_env};

pub struct AcpAdapter;

const EVENTS: &[&str] = &[
    "session_start",
    "user_prompt_submit",
    "pre_tool_use",
    "post_tool_use",
    "permission_request",
    "permission.replied",
    "stop",
    "session_end",
    "compaction_update",
];

impl AcpAdapter {
    fn write_config(
        &self,
        ctx: &SpawnCtx,
        resume_session_id: Option<&str>,
        initial_prompt: Option<&str>,
    ) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(&ctx.run_dir)
            .with_context(|| format!("creating {}", ctx.run_dir.display()))?;
        let environment = base_env(ctx)
            .into_iter()
            .map(|(name, value)| EnvVariable { name, value })
            .collect();
        let config = LaunchConfig {
            version: acp::VERSION,
            system_prompt: ctx.system_prompt.clone(),
            initial_prompt: initial_prompt.map(str::to_string),
            model: ctx.model.clone(),
            effort: ctx.effort.clone(),
            resume_session_id: resume_session_id.map(str::to_string),
            mcp_servers: vec![McpServer {
                name: "ariadne".into(),
                command: ctx.cli_bin.clone(),
                args: vec!["mcp".into(), "serve".into()],
                env: environment,
            }],
            event_sink: Hook {
                command: ctx.cli_bin.clone(),
                args: vec!["agent-event".into(), "--kind".into(), "acp".into()],
            },
        };
        let path = ctx.run_dir.join("acp.json");
        std::fs::write(&path, serde_json::to_string_pretty(&config)?)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    fn plan(
        &self,
        ctx: &SpawnCtx,
        resume_session_id: Option<&str>,
        initial_prompt: Option<&str>,
    ) -> Result<SpawnPlan> {
        let config = self.write_config(ctx, resume_session_id, initial_prompt)?;
        let mut argv = vec!["acp".to_string()];
        argv.extend(ctx.extra_flags.iter().cloned());
        let mut env = base_env(ctx);
        env.push((acp::CONFIG_ENV.into(), config.display().to_string()));
        Ok(SpawnPlan {
            argv,
            env,
            cwd: ctx.cwd.clone(),
            internal_session_id: resume_session_id.map(str::to_string),
            post_launch_input: None,
        })
    }
}

impl AgentAdapter for AcpAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Acp
    }

    fn contract(&self) -> AdapterContract {
        AdapterContract {
            binary: "acp",
            event_kind: "acp",
            generated: &["acp.json"],
            system_prompt: Spelling::Config {
                file: "acp.json",
                pointer: "/systemPrompt",
            },
            model: Spelling::Config {
                file: "acp.json",
                pointer: "/model",
            },
            effort: Spelling::Config {
                file: "acp.json",
                pointer: "/effort",
            },
            skills: Spelling::InThePrompt,
            mcp_command: Spelling::Config {
                file: "acp.json",
                pointer: "/mcpServers/0/command",
            },
            mcp_arguments: Spelling::Config {
                file: "acp.json",
                pointer: "/mcpServers/0/args",
            },
            mcp_arguments_carry_the_command: false,
            mcp_environment: Spelling::Config {
                file: "acp.json",
                pointer: "/mcpServers/0/env",
            },
            events: EventDelivery::Bridge {
                command: Spelling::Config {
                    file: "acp.json",
                    pointer: "/eventSink/command",
                },
                arguments: Spelling::Config {
                    file: "acp.json",
                    pointer: "/eventSink/args",
                },
                events: EVENTS,
            },
            session_id_chosen_at_spawn: false,
            resume_session: Spelling::Config {
                file: "acp.json",
                pointer: "/resumeSessionId",
            },
            resume_instruction: InstructionDelivery::Config(Spelling::Config {
                file: "acp.json",
                pointer: "/initialPrompt",
            }),
            compaction_event: "compaction_update",
        }
    }

    fn plan_spawn(&self, ctx: &SpawnCtx) -> Result<SpawnPlan> {
        self.plan(ctx, None, Some(&ctx.initial_prompt))
    }

    fn plan_resume(
        &self,
        ctx: &SpawnCtx,
        internal_id: &str,
        instruction: &str,
    ) -> Result<SpawnPlan> {
        self.plan(
            ctx,
            Some(internal_id),
            (!instruction.is_empty()).then_some(instruction),
        )
    }

    fn compaction_done(&self, kind: &str, payload: &serde_json::Value) -> bool {
        kind == "compaction_update"
            && payload.get("status").and_then(|value| value.as_str()) == Some("completed")
    }
}
