//! What the agent CLIs are: the flags each is launched with, and the models
//! each can be pointed at.
//!
//! Two endpoints of one subject, and each of them a `list`: an operation id
//! is the handler's name, so the two live in a module apiece rather than
//! sharing one namespace and renaming an id the UI is generated from.

/// Agent-kind configuration endpoints.
///
/// What each coding-agent CLI is launched with, editable in one place: the
/// flags a profile used to carry belong to the agent, not to the persona.
/// Every spawn and resume reads them, so an edit lands on the next launch.
pub mod agents {
    use axum::extract::{Path, State};

    use ariadne_api::agents::{AgentConfigDto, UpdateAgentConfigRequest};
    use ariadne_core::AgentKind;

    use crate::http::AppState;
    use crate::http::convert::agent_config_dto;
    use crate::http::error::{ApiError, ApiResult, Json};

    /// Every agent kind's flags, current and default.
    #[utoipa::path(get, path = "/v1/agents", tag = "agents",
        responses((status = 200, body = [AgentConfigDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AgentConfigDto>>> {
        let configs = state.store.list_agent_configs().await?;
        Ok(Json(configs.into_iter().map(agent_config_dto).collect()))
    }

    /// Replace an agent kind's flags.
    ///
    /// The list is replaced whole, and an empty one is a legitimate answer.
    /// Restoring the defaults is this same call with the `default_flags` the GET
    /// hands out — nothing else to learn, and nothing that can drift from them.
    #[utoipa::path(put, path = "/v1/agents/{kind}", tag = "agents",
        request_body = UpdateAgentConfigRequest,
        params(("kind" = String, Path, description = "claude_code, codex or opencode")),
        responses(
            (status = 200, body = AgentConfigDto),
            (status = 400, description = "unknown agent kind")
        ))]
    pub async fn update(
        State(state): State<AppState>,
        Path(kind): Path<String>,
        Json(req): Json<UpdateAgentConfigRequest>,
    ) -> ApiResult<Json<AgentConfigDto>> {
        let kind = parse_kind(&kind)?;
        let config = state
            .store
            .update_agent_config(kind, req.extra_flags)
            .await?;
        Ok(Json(agent_config_dto(config)))
    }

    /// The agent kind named in the path, or a 400 naming the ones there are.
    fn parse_kind(raw: &str) -> Result<AgentKind, ApiError> {
        raw.parse::<AgentKind>().map_err(|_| {
            let known = AgentKind::ALL
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            ApiError::bad_request(format!(
                "unknown agent kind: {raw} (expected one of {known})"
            ))
        })
    }
}

/// ACP agent registry endpoints.
pub mod acp_agents {
    use axum::extract::State;

    use ariadne_api::agents::AcpAgentDto;

    use crate::http::AppState;
    use crate::http::error::{ApiResult, Json};

    /// Every built-in and configured ACP agent with its cached probe result.
    #[utoipa::path(get, path = "/v1/acp-agents", tag = "acp-agents",
        responses((status = 200, body = [AcpAgentDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<AcpAgentDto>>> {
        Ok(Json(state.agent_registry.agents().await))
    }

    /// Probe every registry entry and replace the cached discovery snapshot.
    #[utoipa::path(post, path = "/v1/acp-agents/refresh", tag = "acp-agents",
        responses((status = 200, body = [AcpAgentDto])))]
    pub async fn refresh(State(state): State<AppState>) -> ApiResult<Json<Vec<AcpAgentDto>>> {
        Ok(Json(state.agent_registry.refresh().await))
    }
}

/// Model catalog endpoint.
pub mod models {
    use std::time::Duration;

    use axum::extract::State;
    use serde::Deserialize;
    use serde::de::{Deserializer, MapAccess, Visitor};

    use ariadne_api::models::{EffortDto, ModelDto, SetModelEnabledRequest};
    use ariadne_core::models::{
        ModelInfo, ModelProfile, ModelRef, curated_models, effort_description, opencode_profile,
    };
    use ariadne_core::{AgentKind, ModelTier};

    use crate::http::AppState;
    use crate::http::error::{ApiError, ApiResult, Json};

    /// Everything an agent can be pinned to, `<agent_kind>:<model>` apiece:
    /// the models of each agent CLI, curated for claude_code and codex,
    /// discovered live (`opencode models --verbose`) for opencode. No
    /// bare-CLI entry: a model is required wherever an agent is pinned, so
    /// there is nothing a CLI on its own could be staffed as.
    ///
    /// The union always, and grouped by agent CLI: a model is chosen by one
    /// string that carries its CLI, so nothing scopes this catalog any more.
    /// Each entry says what the model is for — its tier, its cost and speed
    /// next to every other entry, the work it suits and the work it does not
    /// — and carries the efforts it can be run at, cheapest first, each with
    /// what spending it buys and whether it is the one its CLI runs by
    /// default.
    /// Every entry says whether an agent can be staffed on it. A model the
    /// user turned off stays in the list, off: a catalog that hid it would
    /// leave nothing to turn back on, and nothing to say why a pin naming it
    /// is refused.
    #[utoipa::path(get, path = "/v1/models", tag = "models",
        responses((status = 200, body = [ModelDto])))]
    pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<ModelDto>>> {
        let mut out = catalog(&state).await;
        let off = state.store.disabled_models().await?;
        for entry in &mut out {
            entry.enabled = !off.contains(&entry.id);
        }
        Ok(Json(out))
    }

    /// Turn one entry of the catalog on or off.
    ///
    /// The catalog is code and discovery, so this writes only the exception:
    /// an id nothing in the catalog carries is a 404, and the last entry left
    /// on cannot be turned off — a plan needs something to be staffed on, and
    /// a daemon that can staff nothing is not a state to leave a user in.
    #[utoipa::path(put, path = "/v1/models/enabled", tag = "models",
        request_body = SetModelEnabledRequest,
        responses(
            (status = 200, body = ModelDto),
            (status = 404, description = "no such model in the catalog"),
            (status = 409, description = "it is the last model left enabled")
        ))]
    pub async fn set_enabled(
        State(state): State<AppState>,
        Json(req): Json<SetModelEnabledRequest>,
    ) -> ApiResult<Json<ModelDto>> {
        let mut catalog = catalog(&state).await;
        let off = state.store.disabled_models().await?;
        if !catalog.iter().any(|entry| entry.id == req.id) {
            return Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                format!("no model `{}` in the catalog", req.id),
            ));
        }
        if !req.enabled
            && !catalog
                .iter()
                .any(|entry| entry.id != req.id && !off.contains(&entry.id))
        {
            return Err(ApiError::conflict(format!(
                "`{}` is the last model left enabled — turn another one on first",
                req.id
            )));
        }
        state.store.set_model_enabled(&req.id, req.enabled).await?;
        let mut entry = catalog
            .drain(..)
            .find(|entry| entry.id == req.id)
            .expect("the entry was found above");
        entry.enabled = req.enabled;
        Ok(Json(entry))
    }

    /// The catalog as the CLIs and the discovery describe it, before anything
    /// the user turned off is read over it: every entry comes back `enabled`.
    ///
    /// Native OpenCode discovery uses the configured test seam. ACP discovery
    /// reads the registry cache populated at daemon startup or explicit refresh.
    async fn catalog(state: &AppState) -> Vec<ModelDto> {
        let mut out = Vec::new();
        for kind in AgentKind::ALL {
            match kind {
                AgentKind::Opencode => {
                    out.extend(opencode_models(&state.launcher.cfg.opencode_bin).await)
                }
                _ => out.extend(curated_models(kind).iter().map(|m| curated(kind, m))),
            }
        }
        out.extend(state.agent_registry.models().await);
        out
    }

    /// One curated model as the catalog serves it: what ariadne-core knows
    /// about the model, and the efforts of its CLI it accepts, each described
    /// from that CLI's own ladder.
    fn curated(kind: AgentKind, model: &ModelInfo) -> ModelDto {
        ModelDto {
            id: qualified(kind, model.id),
            agent_id: kind.as_str().to_string(),
            agent_kind: kind,
            description: Some(model.description.to_string()),
            tier: model.tier,
            cost: model.cost,
            speed: model.speed,
            best_for: strings(model.best_for),
            avoid_for: strings(model.avoid_for),
            efforts: model
                .efforts
                .iter()
                .map(|effort| EffortDto {
                    id: (*effort).to_string(),
                    description: effort_description(kind, effort).map(str::to_string),
                    default: model.default_effort == Some(*effort),
                })
                .collect(),
            // Read over by `list`, which is where the user's own choices are.
            enabled: true,
        }
    }

    /// The efforts one model can be run at, as this catalog knows them: the
    /// curated entry for claude_code and codex, and, for opencode, whatever
    /// discovery prints right now.
    ///
    /// None where nothing here lists the model — a hand-typed id — which is
    /// what [`ariadne_core::models::effort_error`] reads as "hold it to
    /// everything that CLI accepts".
    ///
    /// Discovery is re-run rather than cached: it is asked only where a write
    /// names an opencode effort, which is rare, and a remembered list would
    /// start refusing the variants a newly configured model really takes.
    pub async fn efforts_of(kind: AgentKind, model: &str) -> Option<Vec<String>> {
        let names = |efforts: Vec<EffortDto>| efforts.into_iter().map(|e| e.id).collect();
        match kind {
            AgentKind::Opencode => opencode_models(DEFAULT_OPENCODE_BIN)
                .await
                .into_iter()
                .find(|m| m.id == qualified(kind, model))
                .map(|m| names(m.efforts)),
            _ => curated_models(kind)
                .iter()
                .find(|m| m.id == model)
                .map(|m| m.efforts.iter().map(|e| (*e).to_string()).collect()),
        }
    }

    /// What `efforts_of` discovers on, since checking an effort carries no
    /// `AppState` to read `Config::opencode_bin` from. Always the real
    /// binary: nothing here needs the write-time catalog `set_enabled` reads
    /// past a fresh listing, which is what a test seam would be for.
    const DEFAULT_OPENCODE_BIN: &str = "opencode";

    /// OpenCode lists its models natively, and `--verbose` lists what each one
    /// can be run at: a `provider/model` line, then that model's JSON.
    /// Fail-soft: a missing or hung binary yields no models, never an error.
    async fn opencode_models(bin: &str) -> Vec<ModelDto> {
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new(bin)
                .args(["models", "--verbose"])
                .kill_on_drop(true)
                .output(),
        )
        .await;
        let Ok(Ok(output)) = output else {
            return Vec::new();
        };
        discovered(&String::from_utf8_lossy(&output.stdout))
    }

    /// The models `opencode models --verbose` printed, in the order it
    /// printed them, each described from its own block and from whatever
    /// ariadne-core knows about it.
    ///
    /// Fail-soft to the line: a block whose JSON does not parse still yields
    /// the model it belongs to, with nothing said about it — a model that
    /// cannot be described is worth more than a catalog that is missing it.
    fn discovered(stdout: &str) -> Vec<ModelDto> {
        printed(stdout)
            .into_iter()
            .map(|(line, block)| entry(&line, serde_json::from_str(&block).unwrap_or_default()))
            .collect()
    }

    /// The `provider/model` line and the JSON block of every model printed,
    /// paired in the order they were printed.
    fn printed(stdout: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in stdout.lines() {
            // A model's own line is unindented and carries the `/` that
            // separates its provider; everything else is its pretty-printed
            // JSON, which ends on the `}` in the first column.
            if !line.starts_with([' ', '\t', '{', '}']) && line.contains('/') {
                out.push((line.trim().to_string(), String::new()));
                continue;
            }
            let Some((_, block)) = out.last_mut() else {
                continue;
            };
            block.push_str(line);
            block.push('\n');
        }
        out
    }

    /// One discovered model as the catalog serves it: what ariadne-core knows
    /// about it if anything, and otherwise the line its own block affords.
    ///
    /// The id is the line opencode printed, whole, because that is the string
    /// opencode itself takes back; the block only says what the model is.
    fn entry(line: &str, verbose: VerboseModel) -> ModelDto {
        let name = verbose.name().unwrap_or_else(|| line.to_string());
        let known: Option<ModelProfile> = opencode_profile(&name);
        ModelDto {
            id: qualified(AgentKind::Opencode, line),
            agent_id: AgentKind::Opencode.as_str().to_string(),
            agent_kind: AgentKind::Opencode,
            description: match known {
                Some(profile) => Some(profile.description.to_string()),
                None => verbose.describe(&name),
            },
            tier: known.map_or(ModelTier::Unknown, |profile| profile.tier),
            cost: known
                .and_then(|profile| profile.cost)
                .or(verbose.cost_band()),
            speed: known.and_then(|profile| profile.speed),
            best_for: known
                .map(|profile| strings(profile.best_for))
                .unwrap_or_default(),
            avoid_for: known
                .map(|profile| strings(profile.avoid_for))
                .unwrap_or_default(),
            efforts: verbose
                .variants
                .0
                .iter()
                .map(|(id, body)| EffortDto {
                    id: id.clone(),
                    description: body.describe(),
                    // What opencode runs a model at with no variant named is
                    // the model's own configuration, which this does not read.
                    default: false,
                })
                .collect(),
            // Read over by `list`, as for a curated entry.
            enabled: true,
        }
    }

    /// As much of one `--verbose` block as this catalog reads.
    #[derive(Default, Deserialize)]
    struct VerboseModel {
        /// The model's human name, which is the half of a description no id
        /// carries.
        name: Option<String>,
        /// The provider serving it, and the model as that provider names it —
        /// the two halves of the `provider/model` the line spells out.
        #[serde(rename = "providerID")]
        provider_id: Option<String>,
        id: Option<String>,
        cost: Option<Cost>,
        limit: Option<Limit>,
        capabilities: Option<Capabilities>,
        #[serde(default)]
        variants: Variants,
    }

    /// What a model costs, in US dollars per million tokens.
    #[derive(Deserialize)]
    struct Cost {
        input: Option<f64>,
    }

    #[derive(Deserialize)]
    struct Limit {
        context: Option<u64>,
    }

    #[derive(Deserialize)]
    struct Capabilities {
        reasoning: Option<bool>,
    }

    impl VerboseModel {
        /// `provider/model` as the block itself states it, which is the key
        /// anything known about this model is written under.
        fn name(&self) -> Option<String> {
            let provider = self.provider_id.as_deref()?;
            let id = self.id.as_deref()?;
            Some(format!("{provider}/{id}"))
        }

        /// The one line this block affords, for a model nothing has been
        /// written about: what it is called, what it costs, how much it holds
        /// and whether it reasons — and `None` where the block said none of
        /// that.
        fn describe(&self, name: &str) -> Option<String> {
            let mut facts = Vec::new();
            if let Some(band) = self.cost_band() {
                facts.push(price(band).to_string());
            }
            if let Some(context) = self.limit.as_ref().and_then(|l| l.context) {
                facts.push(format!("{} context", tokens(context)));
            }
            match self.capabilities.as_ref().and_then(|c| c.reasoning) {
                Some(true) => facts.push("reasoning".to_string()),
                Some(false) => facts.push("no reasoning".to_string()),
                None => {}
            }
            if facts.is_empty() {
                return None;
            }
            let called = match &self.name {
                Some(called) => format!("{called} ({name})"),
                None => name.to_string(),
            };
            Some(format!("{called}: {}", facts.join(", ")))
        }

        /// The band this model's printed price falls in, on the 1-to-5 scale
        /// the curated entries are ranked on.
        ///
        /// The input price is what the bands are cut on — output tracks it,
        /// and a prompt is most of what an agent sends. Free is 1, which no
        /// paid model reaches; a dollar or less per million is where the
        /// cheap tier sits, five is the everyday tier, fifteen the strong one,
        /// and anything above that is frontier-priced.
        fn cost_band(&self) -> Option<u8> {
            let input = self.cost.as_ref()?.input?;
            Some(match input {
                p if p <= 0.0 => 1,
                p if p <= 1.0 => 2,
                p if p <= 5.0 => 3,
                p if p <= 15.0 => 4,
                _ => 5,
            })
        }
    }

    /// What a cost band is called in a sentence.
    fn price(band: u8) -> &'static str {
        match band {
            1 => "free",
            2 => "inexpensive",
            3 => "mid-priced",
            4 => "expensive",
            _ => "top-priced",
        }
    }

    /// A context window as it is spoken: `200k`, `1M`.
    fn tokens(count: u64) -> String {
        match count {
            n if n >= 1_000_000 => format!("{}M", n / 1_000_000),
            n if n >= 1_000 => format!("{}k", n / 1_000),
            n => n.to_string(),
        }
    }

    /// The variants of one model, kept in the order they were printed:
    /// opencode lists a model's variants cheapest first, which is the order
    /// they are offered in, and sorting them would lose it.
    #[derive(Default)]
    struct Variants(Vec<(String, VariantBody)>);

    /// What one variant sets, of the settings that say something about how
    /// deeply it makes the model think.
    #[derive(Default, Deserialize)]
    struct VariantBody {
        #[serde(rename = "reasoningEffort")]
        reasoning_effort: Option<String>,
        thinking: Option<Thinking>,
    }

    #[derive(Deserialize)]
    struct Thinking {
        #[serde(rename = "budgetTokens")]
        budget_tokens: Option<u64>,
    }

    impl VariantBody {
        /// What running this variant does, in the terms the variant itself is
        /// written in — and `None` where its body says nothing that bears on
        /// how hard the model thinks.
        fn describe(&self) -> Option<String> {
            let mut set = Vec::new();
            if let Some(effort) = &self.reasoning_effort {
                set.push(format!("reasoningEffort={effort}"));
            }
            if let Some(budget) = self.thinking.as_ref().and_then(|t| t.budget_tokens) {
                set.push(format!("a thinking budget of {} tokens", tokens(budget)));
            }
            (!set.is_empty()).then(|| set.join(", "))
        }
    }

    impl<'de> Deserialize<'de> for Variants {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Entries;

            impl<'de> Visitor<'de> for Entries {
                type Value = Variants;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a map of variants")
                }

                fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                    let mut variants = Vec::new();
                    while let Some(entry) = map.next_entry::<String, VariantBody>()? {
                        variants.push(entry);
                    }
                    Ok(Variants(variants))
                }
            }

            deserializer.deserialize_map(Entries)
        }
    }

    /// One catalog entry's id: the model as its agent CLI runs it, which is
    /// what a request writes to pin it.
    fn qualified(kind: AgentKind, model: &str) -> String {
        ModelRef {
            agent_kind: kind,
            model: model.to_string(),
        }
        .to_string()
    }

    /// A curated list of task shapes, as the wire carries them.
    fn strings(shapes: &[&str]) -> Vec<String> {
        shapes.iter().map(|s| (*s).to_string()).collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// What `opencode models --verbose` prints, cut to what is read: a
        /// model ariadne-core knows, one it does not, one whose variants set a
        /// thinking budget rather than an effort, and one whose block is not
        /// JSON at all.
        const VERBOSE: &str = r#"opencode/hy3-free
{
  "id": "hy3-free",
  "providerID": "opencode",
  "name": "Hy3 Free",
  "cost": {
    "input": 0,
    "output": 0
  },
  "limit": {
    "context": 190000
  },
  "capabilities": {
    "reasoning": true
  },
  "variants": {
    "low": {
      "reasoningEffort": "low"
    },
    "high": {
      "reasoningEffort": "high"
    }
  }
}
someone/thinker-2
{
  "id": "thinker-2",
  "providerID": "someone",
  "name": "Thinker 2",
  "cost": {
    "input": 3,
    "output": 15
  },
  "limit": {
    "context": 1048576
  },
  "capabilities": {
    "reasoning": true
  },
  "variants": {
    "deep": {
      "thinking": {
        "budgetTokens": 32000
      }
    },
    "plain": {}
  }
}
ollama/qwen3.6-code
{
  "id": "qwen3.6-code",
  truncated by a
}
"#;

        fn found<'a>(models: &'a [ModelDto], id: &str) -> &'a ModelDto {
            models
                .iter()
                .find(|m| m.id == id)
                .unwrap_or_else(|| panic!("missing {id}"))
        }

        /// Every model printed is offered, each under the id it is pinned by,
        /// with the variants it was printed with in the order they came.
        #[test]
        fn discovery_reads_each_model_and_the_variants_it_runs_at() {
            let got = discovered(VERBOSE);
            let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
            assert_eq!(
                ids,
                [
                    "opencode:opencode/hy3-free",
                    "opencode:someone/thinker-2",
                    "opencode:ollama/qwen3.6-code",
                ]
            );
            let efforts =
                |m: &ModelDto| -> Vec<String> { m.efforts.iter().map(|e| e.id.clone()).collect() };
            assert_eq!(efforts(&got[0]), ["low", "high"], "as they were printed");
            assert_eq!(efforts(&got[1]), ["deep", "plain"]);
            assert!(efforts(&got[2]).is_empty(), "unreadable block");
            assert!(
                got.iter().all(|m| m.efforts.iter().all(|e| !e.default)),
                "what opencode runs a model at is its own configuration"
            );
        }

        /// A model nothing has been written about is described from its own
        /// block: what it is called, what it costs, how much it holds and
        /// whether it reasons — and its variants from what they set.
        #[test]
        fn an_unknown_model_is_described_from_what_was_printed() {
            let got = discovered(VERBOSE);
            let thinker = found(&got, "opencode:someone/thinker-2");
            assert_eq!(
                thinker.description.as_deref(),
                Some("Thinker 2 (someone/thinker-2): mid-priced, 1M context, reasoning")
            );
            assert_eq!(thinker.tier, ModelTier::Unknown);
            assert_eq!(thinker.cost, Some(3), "$3 per million input tokens");
            assert_eq!(thinker.speed, None);
            assert!(thinker.best_for.is_empty() && thinker.avoid_for.is_empty());
            assert_eq!(
                thinker.efforts[0].description.as_deref(),
                Some("a thinking budget of 32k tokens")
            );
            assert_eq!(
                thinker.efforts[1].description, None,
                "a variant whose body says nothing about thinking"
            );
        }

        /// A model ariadne-core knows is served as ariadne-core has it, over
        /// the line discovery could have derived; the efforts stay the ones
        /// discovery printed, since only opencode knows what it will accept.
        #[test]
        fn a_known_model_is_served_as_the_catalog_has_it() {
            let got = discovered(VERBOSE);
            let hy3 = found(&got, "opencode:opencode/hy3-free");
            let known = opencode_profile("opencode/hy3-free").expect("curated");
            assert_eq!(hy3.description.as_deref(), Some(known.description));
            assert_eq!(hy3.tier, known.tier);
            assert_eq!(hy3.cost, Some(1), "free");
            assert_eq!(hy3.best_for, strings(known.best_for));
            assert_eq!(hy3.avoid_for, strings(known.avoid_for));
            assert_eq!(
                hy3.efforts[1].description.as_deref(),
                Some("reasoningEffort=high")
            );
        }

        /// A block that does not parse still yields the model it belongs to,
        /// with nothing claimed about it.
        #[test]
        fn an_unreadable_block_still_yields_its_model() {
            let got = discovered(VERBOSE);
            let qwen = found(&got, "opencode:ollama/qwen3.6-code");
            // The line is the key, so what is known about this model is found
            // even where its block was lost.
            assert_eq!(
                qwen.description.as_deref(),
                opencode_profile("ollama/qwen3.6-code").map(|p| p.description)
            );
            assert!(qwen.efforts.is_empty());
        }

        /// Nothing at all — no binary, or a version that prints nothing — is
        /// no models rather than a half-read one.
        #[test]
        fn discovery_of_nothing_is_no_models() {
            assert!(discovered("").is_empty());
            assert!(discovered("\n\n").is_empty());
        }

        /// A curated model carries what its CLI's ladder says about each of
        /// its efforts, and flags the one the CLI runs it at.
        #[test]
        fn a_curated_model_carries_its_clis_ladder() {
            let opus = curated_models(AgentKind::ClaudeCode)
                .iter()
                .find(|m| m.id == "claude-opus-4-7")
                .expect("curated");
            let dto = curated(AgentKind::ClaudeCode, opus);
            assert_eq!(dto.id, "claude_code:claude-opus-4-7");
            assert_eq!(dto.tier, opus.tier);
            let defaults: Vec<_> = dto
                .efforts
                .iter()
                .filter(|e| e.default)
                .map(|e| e.id.as_str())
                .collect();
            assert_eq!(defaults, ["xhigh"]);
            assert!(dto.efforts.iter().all(|e| e.description.is_some()));
        }
    }
}
