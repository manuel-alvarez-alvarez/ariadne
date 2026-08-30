//! `ariadne doctor` — one report on the whole installation.
//!
//! Every check is a line with a verdict (`ok`, `warn`, `fail`) and, when
//! something is wrong, the one thing to do about it. Nothing here changes
//! anything: doctor diagnoses, and says what would fix it.
//!
//! The report has two halves on purpose. What this shell sees is one thing;
//! what `ariadned` sees is another, and it is the one that matters, because
//! the daemon is what spawns sessions. A daemon started by launchd or systemd
//! carries the PATH its service file was written with, so an agent CLI
//! installed after the install can be perfectly present here and invisible
//! there — which looks, from the outside, like a profile that will not run.

mod agents;
mod checks;

use std::process::ExitCode;

use anstyle::Style;
use anyhow::Result;
use serde::Serialize;

use ariadne_api::doctor::{BinaryDto, DaemonReportDto};
use ariadne_api::profiles::ProfileDto;
use ariadne_client::{Client, endpoint};
use ariadne_core::AgentKind;

use crate::output::{Format, View, note, print, style, view};

/// Which agent binaries can actually be launched, from both points of view.
#[derive(Debug, Default, Clone)]
pub struct Availability {
    /// Kinds on the daemon's PATH; `None` when no daemon answered.
    daemon: Option<Vec<AgentKind>>,
    /// Kinds on this shell's PATH.
    client: Vec<AgentKind>,
}

impl Availability {
    fn new(daemon: Option<&DaemonReportDto>, agents: &[BinaryDto]) -> Self {
        let launchable = |binaries: &[BinaryDto]| -> Vec<AgentKind> {
            binaries
                .iter()
                .filter(|b| b.path.is_some())
                .filter_map(|b| b.agent_kind)
                .collect()
        };
        Self {
            daemon: daemon.map(|d| launchable(&d.agents)),
            client: launchable(agents),
        }
    }

    /// What the process that spawns sessions can launch: the daemon's view
    /// when there is one, this shell's as the only stand-in when there is not.
    pub(super) fn effective(&self) -> &[AgentKind] {
        self.daemon.as_deref().unwrap_or(&self.client)
    }

    pub(super) fn has(&self, kind: AgentKind) -> bool {
        self.effective().contains(&kind)
    }

    /// Installed here but not where it counts — the shape a stale service
    /// PATH takes.
    pub(super) fn only_on_client(&self, kind: AgentKind) -> bool {
        self.daemon.is_some() && !self.has(kind) && self.client.contains(&kind)
    }

    /// Nothing to launch where it counts, while this shell has agents: the
    /// same stale service PATH, seen across all three at once.
    pub(super) fn stale_service_path(&self) -> bool {
        self.daemon.is_some() && self.effective().is_empty() && !self.client.is_empty()
    }

    /// Whose PATH a verdict was reached on, so a failure says where to look.
    pub(super) fn viewpoint(&self) -> &'static str {
        match self.daemon.is_some() {
            true => checks::THERE,
            false => checks::HERE,
        }
    }
}

/// How a check came out. Ordered by severity so the worst one wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// One line of the report: what was checked, how it came out, and — when it
/// did not come out well — what to do next.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Check {
    name: String,
    status: Status,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

impl Check {
    fn new(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
            hint: None,
        }
    }

    fn ok(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Ok, detail)
    }

    fn warn(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Warn, detail)
    }

    fn fail(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(name, Status::Fail, detail)
    }

    fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The shape most checks have: the same sentence either way, `ok` when the
    /// answer is yes and `bad` — with the one thing to do about it — when not.
    fn when(
        name: impl Into<String>,
        good: bool,
        detail: impl Into<String>,
        bad: Status,
        hint: impl Into<String>,
    ) -> Self {
        match good {
            true => Self::ok(name, detail),
            false => Self::new(name, bad, detail).hint(hint),
        }
    }
}

/// A group of checks, rendered under one heading.
#[derive(Debug, Clone, Serialize)]
struct Section {
    name: String,
    checks: Vec<Check>,
}

impl Section {
    fn new(name: impl Into<String>, checks: Vec<Check>) -> Self {
        Self {
            name: name.into(),
            checks,
        }
    }
}

/// The whole report, and the verdict the exit code comes from.
#[derive(Debug, Clone, Serialize)]
struct Report {
    /// The worst status of any check.
    status: Status,
    /// The daemon this was measured against.
    endpoint: String,
    sections: Vec<Section>,
}

impl Report {
    fn new(endpoint: impl Into<String>, sections: Vec<Section>) -> Self {
        Self {
            status: sections
                .iter()
                .flat_map(|s| s.checks.iter())
                .map(|c| c.status)
                .max()
                .unwrap_or(Status::Ok),
            endpoint: endpoint.into(),
            sections,
        }
    }

    /// Checks at a given status, for the closing verdict.
    fn count(&self, status: Status) -> usize {
        self.sections
            .iter()
            .flat_map(|s| s.checks.iter())
            .filter(|c| c.status == status)
            .count()
    }

    /// 1 as soon as anything failed, 0 otherwise: warnings are things to look
    /// at, failures are things that stop Ariadne working.
    fn exit_code(&self) -> ExitCode {
        match self.status {
            Status::Fail => ExitCode::from(1),
            _ => ExitCode::SUCCESS,
        }
    }
}

/// `ariadne doctor` — build the report, print it, and answer with it.
pub async fn run(client: &Client, format: Format) -> Result<ExitCode> {
    let report = examine(client).await;
    let view = view();
    print(format, &report, || {
        for line in render(&report, view) {
            println!("{line}");
        }
        let verdict_style = style::check(report.status.label()).0;
        note(&style::paint(view.color, verdict_style, &summary(&report)));
    })?;
    Ok(report.exit_code())
}

/// Ask the whole installation how it is doing.
///
/// One pass, whether or not there is a daemon to ask: a stopped daemon is one
/// failing check inside a full report, not a command that gives up. What
/// needed the daemon is marked unmeasured; the binaries, the home and the
/// service registration — exactly what one wants when the daemon is down —
/// are measured as usual.
async fn examine(client: &Client) -> Report {
    let home = endpoint::home(None);
    let config = home.as_deref().map(endpoint::parse_config);

    // Probes are processes: run them at once rather than three seconds apart.
    let (claude, codex, opencode, tmux, git, gh, glab, ariadned) = tokio::join!(
        checks::agent(AgentKind::ClaudeCode),
        checks::agent(AgentKind::Codex),
        checks::agent(AgentKind::Opencode),
        checks::tool("tmux", "-V", false),
        checks::tool("git", "--version", false),
        checks::tool("gh", "--version", true),
        checks::tool("glab", "--version", true),
        checks::ariadned(),
    );
    let agents = vec![claude, codex, opencode];

    let health = client.health().await;
    let reachable = health.is_ok();
    // Everything here only exists for a daemon that answered at all.
    let (version, daemon, profiles, flags) = match reachable {
        true => {
            let (version, daemon, profiles, flags) = tokio::join!(
                client.version(),
                client.daemon_report(),
                client.get_json::<Vec<ProfileDto>>("/v1/profiles"),
                client.list_agent_configs(),
            );
            (
                version.ok().map(|v| v.version),
                daemon.ok(),
                profiles.unwrap_or_default(),
                flags.unwrap_or_default(),
            )
        }
        false => (None, None, Vec::new(), Vec::new()),
    };
    let available = Availability::new(daemon.as_ref(), &agents);

    Report::new(
        client.endpoint(),
        vec![
            Section::new("client", checks::client(&ariadned, reachable)),
            Section::new("home", checks::home(home.as_deref(), config).await),
            Section::new(
                "daemon",
                checks::daemon(client, &health, version, home.as_deref()).await,
            ),
            Section::new("tools", checks::tools(&[tmux, git], &[gh, glab])),
            Section::new(
                "agents",
                agents::agents(&agents, &flags, &available, &profiles),
            ),
            Section::new(
                "profiles",
                match reachable {
                    true => agents::profiles(&profiles, &available),
                    // Nothing was listed, which is not the same as no profiles.
                    false => vec![Check::warn(
                        "profiles",
                        "not checked — the daemon did not answer",
                    )],
                },
            ),
            Section::new(
                "daemon environment",
                agents::daemon_environment(daemon.as_ref(), &available),
            ),
        ],
    )
}

/// A check's status word, painted in `style::check`'s colour and prefixed
/// with its glyph: `✓ ok`, `! warn`, `✗ fail`.
fn verdict(status: Status) -> (Style, String) {
    let (style, glyph) = style::check(status.label());
    let text = match glyph {
        Some(glyph) => format!("{glyph} {}", status.label()),
        None => status.label().to_string(),
    };
    (style, text)
}

/// The report as lines: a heading per section, a line per check, and the hint
/// of anything that is not `ok` under it.
///
/// The status column is padded on its unpainted text — glyph included — so
/// the escapes colour adds never throw off `name` and `detail`, which start
/// at the same offset whatever the verdict.
fn render(report: &Report, view: &View) -> Vec<String> {
    let checks: Vec<&Check> = report
        .sections
        .iter()
        .flat_map(|s| s.checks.iter())
        .collect();
    let name_width = checks
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0);
    let status_width = checks
        .iter()
        .map(|c| verdict(c.status).1.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    for section in &report.sections {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(style::paint(
            view.color,
            style::HEADING,
            &section.name.to_uppercase(),
        ));
        for check in &section.checks {
            let (style, text) = verdict(check.status);
            let painted = style::paint(view.color, style, &text);
            let pad = " ".repeat(status_width - text.chars().count() + 2);
            lines.push(format!(
                "  {painted}{pad}{:<name_width$}   {}",
                check.name, check.detail
            ));
            if let Some(hint) = &check.hint {
                let hint = style::paint(view.color, style::META, hint);
                lines.push(format!(
                    "  {:<status_width$}{:<name_width$}   {hint}",
                    "",
                    "",
                    status_width = status_width + 2
                ));
            }
        }
    }
    lines
}

/// The one line that says whether to act on any of this.
fn summary(report: &Report) -> String {
    match (report.count(Status::Fail), report.count(Status::Warn)) {
        (0, 0) => "everything checks out".to_string(),
        (0, w) => format!("{w} warning(s) — nothing broken"),
        (f, 0) => format!("{f} failure(s) — Ariadne will not work until they are fixed"),
        (f, w) => format!("{f} failure(s) and {w} warning(s)"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use ariadne_api::doctor::BinaryDto;

    /// A binary as either half of the report carries one.
    pub(crate) fn binary(
        name: &str,
        kind: Option<AgentKind>,
        found: bool,
        auth: Option<bool>,
    ) -> BinaryDto {
        BinaryDto {
            name: name.into(),
            agent_kind: kind,
            path: found.then(|| format!("/bin/{name}")),
            version: found.then(|| "1.0".to_string()),
            authenticated: auth,
        }
    }

    pub(crate) fn by_name<'a>(checks: &'a [Check], name: &str) -> &'a Check {
        checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no {name} check"))
    }

    fn report(checks: Vec<Check>) -> Report {
        Report::new("/tmp/ariadne.sock", vec![Section::new("s", checks)])
    }

    /// The report takes the worst status of its checks, and only a failure —
    /// a broken install, as against something to look at — exits nonzero.
    #[test]
    fn the_report_takes_the_worst_status_and_only_a_failure_exits_nonzero() {
        let code = |checks: Vec<Check>| format!("{:?}", report(checks).exit_code());
        let success = format!("{:?}", ExitCode::SUCCESS);

        assert_eq!(report(vec![]).status, Status::Ok);
        assert_eq!(code(vec![]), success);

        let warned = vec![Check::ok("a", "-"), Check::warn("b", "-")];
        assert_eq!(report(warned.clone()).status, Status::Warn);
        assert_eq!(code(warned), success);

        let failed = vec![Check::warn("a", "-"), Check::fail("b", "-")];
        assert_eq!(report(failed.clone()).status, Status::Fail);
        assert_eq!(code(failed), format!("{:?}", ExitCode::from(1)));
    }

    #[test]
    fn every_check_prints_under_its_section_with_its_hint() {
        let lines = render(
            &Report::new(
                "/tmp/ariadne.sock",
                vec![Section::new(
                    "client",
                    vec![Check::fail("ariadned", "not found").hint("install it")],
                )],
            ),
            &View::plain(),
        );
        assert_eq!(lines[0], "CLIENT");
        assert!(lines[1].contains("✗ fail"), "{lines:?}");
        assert!(lines[1].contains("ariadned"), "{lines:?}");
        assert!(lines[2].contains("install it"), "{lines:?}");
        assert!(!lines.join("\n").contains('\u{1b}'), "{lines:?}");
    }

    /// With colour on: the section heading is bold, each status word carries
    /// its glyph in `check()`'s colour, a hint is dimmed — and the `name`
    /// column starts at the same offset whether the row above it says `ok`,
    /// `warn` or `fail`. With colour off, the same report differs only by
    /// the glyphs the status words now carry.
    #[test]
    fn the_coloured_report_paints_headings_verdicts_and_hints_and_keeps_columns_aligned() {
        let report = Report::new(
            "/tmp/ariadne.sock",
            vec![Section::new(
                "tools",
                vec![
                    Check::ok("git", "2.43.0"),
                    Check::warn("gh", "not authenticated"),
                    Check::fail("tmux", "not found").hint("install tmux"),
                ],
            )],
        );
        let colour = View {
            color: true,
            ..View::plain()
        };
        let lines = render(&report, &colour);
        let [heading, ok_line, warn_line, fail_line, hint_line]: [&String; 5] =
            lines.iter().collect::<Vec<_>>().try_into().unwrap();

        assert_eq!(*heading, style::paint(true, style::HEADING, "TOOLS"));
        assert!(
            ok_line.contains(&style::paint(true, style::check("ok").0, "✓ ok")),
            "{ok_line}"
        );
        assert!(
            warn_line.contains(&style::paint(true, style::check("warn").0, "! warn")),
            "{warn_line}"
        );
        assert!(
            fail_line.contains(&style::paint(true, style::check("fail").0, "✗ fail")),
            "{fail_line}"
        );
        assert!(
            hint_line.contains(&style::paint(true, style::META, "install tmux")),
            "{hint_line}"
        );

        // `name` starts in the same column on every row, escapes and all —
        // a character offset, since a multi-byte glyph like `✗` would throw
        // a byte offset off despite costing the terminal a single column.
        let name_offset = |line: &str, name: &str| {
            let plain = visible(line);
            plain.find(name).map(|byte| plain[..byte].chars().count())
        };
        assert_eq!(name_offset(ok_line, "git"), name_offset(warn_line, "gh"));
        assert_eq!(name_offset(ok_line, "git"), name_offset(fail_line, "tmux"));

        // Plain is the same report, minus every escape — and the glyphs,
        // which are the one thing colour is allowed to have added.
        let plain = render(&report, &View::plain());
        let stripped: Vec<String> = lines.iter().map(|l| visible(l)).collect();
        assert_eq!(stripped, plain);
        for line in &plain {
            assert!(!line.contains('\u{1b}'), "{line:?}");
        }
        assert!(plain[1].contains("✓ ok"), "{plain:?}");
        assert!(plain[2].contains("! warn"), "{plain:?}");
        assert!(plain[3].contains("✗ fail"), "{plain:?}");
    }

    /// A line as the reader sees it: the escapes taken back out.
    fn visible(line: &str) -> String {
        let mut out = String::new();
        let mut escaped = false;
        for c in line.chars() {
            match (escaped, c) {
                (false, '\u{1b}') => escaped = true,
                (true, 'm') => escaped = false,
                (true, _) => {}
                (false, c) => out.push(c),
            }
        }
        out
    }

    #[test]
    fn the_summary_separates_warnings_from_failures() {
        assert_eq!(
            summary(&report(vec![Check::ok("a", "-")])),
            "everything checks out"
        );
        assert!(summary(&report(vec![Check::warn("a", "-")])).contains("nothing broken"));
        assert!(summary(&report(vec![Check::fail("a", "-")])).contains("1 failure"));
    }

    /// `--format json` has to carry every check with its status, or a script
    /// cannot act on the report.
    #[test]
    fn the_json_report_carries_every_check_and_its_status() {
        let json = serde_json::to_value(Report::new(
            "/tmp/ariadne.sock",
            vec![Section::new(
                "tools",
                vec![Check::fail("tmux", "not found").hint("install tmux")],
            )],
        ))
        .unwrap();
        assert_eq!(json["status"], "fail");
        assert_eq!(json["sections"][0]["name"], "tools");
        assert_eq!(json["sections"][0]["checks"][0]["status"], "fail");
        assert_eq!(json["sections"][0]["checks"][0]["hint"], "install tmux");
    }
}
