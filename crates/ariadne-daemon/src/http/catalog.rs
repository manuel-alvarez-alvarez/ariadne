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
    use axum::Json;
    use axum::extract::{Path, State};

    use ariadne_api::agents::{AgentConfigDto, UpdateAgentConfigRequest};
    use ariadne_core::AgentKind;

    use crate::http::AppState;
    use crate::http::convert::agent_config_dto;
    use crate::http::error::{ApiError, ApiResult};

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

/// Model catalog endpoint.
pub mod models {
    use std::time::Duration;

    use axum::Json;
    use serde::Deserialize;
    use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};

    use ariadne_api::models::ModelDto;
    use ariadne_core::AgentKind;
    use ariadne_core::models::{ModelRef, curated_models};

    /// Everything an agent can be pinned to, `<agent_kind>[:<model>]` apiece:
    /// each agent CLI on its own — that CLI on its own default model — and
    /// then the models of it, curated for claude_code and codex, discovered
    /// live (`opencode models --verbose`) for opencode.
    ///
    /// The union always, and grouped by agent CLI: a model is chosen by one
    /// string that carries its CLI, so nothing scopes this catalog any more.
    /// Each entry carries the reasoning efforts it can be run at, cheapest
    /// first, and what its CLI runs it at when none is passed.
    #[utoipa::path(get, path = "/v1/models", tag = "models",
        responses((status = 200, body = [ModelDto])))]
    pub async fn list() -> Json<Vec<ModelDto>> {
        let mut out = Vec::new();
        for kind in AgentKind::ALL {
            out.push(ModelDto {
                id: ModelRef::of(kind).to_string(),
                agent_kind: kind,
                description: Some(format!("{} on its own default model", kind.as_str())),
                // Which model that is, is the CLI's own business, so what it
                // is run at is not this catalog's to say.
                efforts: Vec::new(),
                default_effort: None,
            });
            match kind {
                AgentKind::Opencode => out.extend(opencode_models().await),
                _ => out.extend(curated_models(kind).iter().map(|m| ModelDto {
                    id: qualified(kind, m.id),
                    agent_kind: kind,
                    description: Some(m.description.to_string()),
                    efforts: m.efforts.iter().map(|e| e.to_string()).collect(),
                    default_effort: m.default_effort.map(str::to_string),
                })),
            }
        }
        Json(out)
    }

    /// The efforts one model can be run at, as this catalog knows them: the
    /// curated entry for claude_code and codex, and, for opencode, whatever
    /// discovery prints right now.
    ///
    /// None where nothing here lists the model — a hand-typed id, or an agent
    /// CLI on its own default model — which is what
    /// [`ariadne_core::models::effort_error`] reads as "hold it to everything
    /// that CLI accepts".
    ///
    /// Discovery is re-run rather than cached: it is asked only where a write
    /// names an opencode effort, which is rare, and a remembered list would
    /// start refusing the variants a newly configured model really takes.
    pub async fn efforts_of(kind: AgentKind, model: Option<&str>) -> Option<Vec<String>> {
        let model = model?;
        match kind {
            AgentKind::Opencode => opencode_models()
                .await
                .into_iter()
                .find(|m| m.id == qualified(kind, model))
                .map(|m| m.efforts),
            _ => curated_models(kind)
                .iter()
                .find(|m| m.id == model)
                .map(|m| m.efforts.iter().map(|e| e.to_string()).collect()),
        }
    }

    /// OpenCode lists its models natively, and `--verbose` lists what each one
    /// can be run at: a `provider/model` line, then that model's JSON.
    /// Fail-soft: a missing or hung binary yields no models, never an error.
    async fn opencode_models() -> Vec<ModelDto> {
        let output = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new("opencode")
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

    /// The models `opencode models --verbose` printed, each with the efforts
    /// its `variants` are named by, in the order it printed them.
    ///
    /// Fail-soft to the line: a block whose JSON does not parse still yields
    /// the model it belongs to, with no efforts — a model that cannot be run
    /// at a named variant is worth more than a catalog that is missing it.
    fn discovered(stdout: &str) -> Vec<ModelDto> {
        let mut out: Vec<ModelDto> = Vec::new();
        let mut block = String::new();
        for line in stdout.lines() {
            // A model's own line is unindented and carries the `/` that
            // separates its provider; everything else is its pretty-printed
            // JSON, which ends on the `}` in the first column.
            if !line.starts_with([' ', '\t', '{', '}']) && line.contains('/') {
                out.push(ModelDto {
                    id: qualified(AgentKind::Opencode, line.trim()),
                    agent_kind: AgentKind::Opencode,
                    description: None,
                    efforts: Vec::new(),
                    default_effort: None,
                });
                block.clear();
                continue;
            }
            let Some(model) = out.last_mut() else {
                continue;
            };
            block.push_str(line);
            block.push('\n');
            if line == "}" {
                model.efforts = variants(&block);
                block.clear();
            }
        }
        out
    }

    /// The variant names in one model's JSON, in the order it lists them, and
    /// none where it lists none or where the block is not JSON at all.
    fn variants(block: &str) -> Vec<String> {
        serde_json::from_str::<VerboseModel>(block)
            .map(|m| m.variants.0)
            .unwrap_or_default()
    }

    /// As much of one `--verbose` block as this catalog reads.
    #[derive(Deserialize)]
    struct VerboseModel {
        #[serde(default)]
        variants: VariantNames,
    }

    /// The keys of the `variants` map, kept in the order they were printed:
    /// opencode lists a model's variants cheapest first, which is the order
    /// they are offered in, and sorting them would lose it.
    #[derive(Default)]
    struct VariantNames(Vec<String>);

    impl<'de> Deserialize<'de> for VariantNames {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Keys;

            impl<'de> Visitor<'de> for Keys {
                type Value = VariantNames;

                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a map of variants")
                }

                fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                    let mut names = Vec::new();
                    while let Some(name) = map.next_key::<String>()? {
                        map.next_value::<IgnoredAny>()?;
                        names.push(name);
                    }
                    Ok(VariantNames(names))
                }
            }

            deserializer.deserialize_map(Keys)
        }
    }

    /// One catalog entry's id: the model as its agent CLI runs it, which is
    /// what a request writes to pin it.
    fn qualified(kind: AgentKind, model: &str) -> String {
        ModelRef {
            agent_kind: kind,
            model: Some(model.to_string()),
        }
        .to_string()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// What `opencode models --verbose` prints, cut to what is read: a
        /// model with variants, one with none, and one whose block is not
        /// JSON at all.
        const VERBOSE: &str = r#"opencode/big-pickle
{
  "id": "big-pickle",
  "variants": {}
}
openai/gpt-5.6-terra
{
  "id": "gpt-5.6-terra",
  "variants": {
    "low": {
      "reasoningEffort": "low"
    },
    "high": {
      "reasoningEffort": "high"
    }
  }
}
ollama/qwen3.6-code
{
  "id": "qwen3.6-code",
  truncated by a
}
"#;

        /// Every model printed is offered, each under the id it is pinned by,
        /// with the variants it was printed with in the order they came.
        #[test]
        fn discovery_reads_each_model_and_the_variants_it_runs_at() {
            let got = discovered(VERBOSE);
            let ids: Vec<_> = got.iter().map(|m| m.id.as_str()).collect();
            assert_eq!(
                ids,
                [
                    "opencode:opencode/big-pickle",
                    "opencode:openai/gpt-5.6-terra",
                    "opencode:ollama/qwen3.6-code",
                ]
            );
            assert_eq!(got[0].efforts, [] as [String; 0], "no variants at all");
            assert_eq!(got[1].efforts, ["low", "high"], "as they were printed");
            assert_eq!(got[2].efforts, [] as [String; 0], "unreadable block");
            assert!(got.iter().all(|m| m.default_effort.is_none()));
            assert!(got.iter().all(|m| m.description.is_none()));
        }

        /// Nothing at all — no binary, or a version that prints nothing — is
        /// no models rather than a half-read one.
        #[test]
        fn discovery_of_nothing_is_no_models() {
            assert!(discovered("").is_empty());
            assert!(discovered("\n\n").is_empty());
        }
    }
}
