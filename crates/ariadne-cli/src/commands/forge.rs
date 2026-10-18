//! `ariadne forge tunnel [on|off]`.

use anyhow::Result;
use clap::Subcommand;

use ariadne_api::repositories::{ForgeTunnelDto, SetTunnelRequest, TunnelState};
use ariadne_client::Client;

use super::repo::Switch;
use crate::output::{Format, print, print_kv};

const TUNNEL: &str = "/v1/forge/tunnel";

#[derive(Subcommand)]
pub(crate) enum ForgeCommand {
    /// Show the webhook tunnel, or turn it on or off
    ///
    /// Off closes the tunnel, and every enabled repository fetches on its
    /// timer. The hooks stay registered.
    Tunnel {
        /// Turn the tunnel on or off
        #[arg(value_enum)]
        switch: Option<Switch>,
    },
}

pub(crate) async fn run(client: &Client, command: ForgeCommand, format: Format) -> Result<()> {
    let ForgeCommand::Tunnel { switch } = command;
    let tunnel: ForgeTunnelDto = match switch {
        None => client.get_json(TUNNEL).await?,
        Some(switch) => {
            client
                .put_json(
                    TUNNEL,
                    &SetTunnelRequest {
                        enabled: switch == Switch::On,
                    },
                )
                .await?
        }
    };
    print(format, &tunnel, || print_kv(&rows(&tunnel)))
}

/// The tunnel in a few words: `up https://x.loca.lt`, `down`, `off`.
pub(crate) fn label(tunnel: &ForgeTunnelDto) -> String {
    let state = match tunnel.state {
        TunnelState::Up => "up",
        TunnelState::Down => "down",
        TunnelState::Off => "off",
    };
    match &tunnel.url {
        Some(url) => format!("{state} {url}"),
        None => state.into(),
    }
}

fn rows(tunnel: &ForgeTunnelDto) -> Vec<(&'static str, String)> {
    let dash = |value: &Option<String>| value.clone().unwrap_or_else(|| "-".into());
    vec![
        ("switch", if tunnel.enabled { "on" } else { "off" }.into()),
        ("state", label(tunnel)),
        ("listen", dash(&tunnel.listen)),
        ("since", tunnel.since.clone()),
        ("error", dash(&tunnel.error)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};
    use std::sync::{Arc, Mutex};

    pub(crate) fn tunnel(enabled: bool) -> ForgeTunnelDto {
        ForgeTunnelDto {
            enabled,
            state: if enabled {
                TunnelState::Up
            } else {
                TunnelState::Off
            },
            url: enabled.then(|| "https://amber-104233.loca.lt".into()),
            listen: Some("127.0.0.1:49152".into()),
            since: "2026-10-08T10:00:00.000Z".into(),
            error: None,
        }
    }

    #[test]
    fn the_tunnel_prints_its_state_url_and_bound_address() {
        let block = crate::output::kv_block(&rows(&tunnel(true)), &crate::output::View::plain());
        assert!(block.contains("up https://amber-104233.loca.lt"), "{block}");
        assert!(block.contains("127.0.0.1:49152"), "{block}");
        assert!(
            block
                .lines()
                .any(|l| l.starts_with("switch") && l.trim_end().ends_with("on"))
        );
    }

    #[tokio::test]
    async fn forge_tunnel_on_and_off_set_the_switch() {
        let bodies = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let seen = bodies.clone();
        let app = Router::new().route(
            TUNNEL,
            get(|| async { axum::Json(tunnel(true)) }).put(
                move |axum::Json(body): axum::Json<serde_json::Value>| {
                    let seen = seen.clone();
                    async move {
                        let enabled = body["enabled"].as_bool().unwrap();
                        seen.lock().unwrap().push(body);
                        axum::Json(tunnel(enabled))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = Client::tcp(format!("http://{address}"));
        for switch in [Some(Switch::Off), Some(Switch::On), None] {
            run(&client, ForgeCommand::Tunnel { switch }, Format::Json)
                .await
                .unwrap();
        }
        server.abort();
        assert_eq!(
            *bodies.lock().unwrap(),
            [
                serde_json::json!({"enabled": false}),
                serde_json::json!({"enabled": true})
            ]
        );
    }
}
