//! The request column opens, keeps, and ends a stepped task's request.

use std::path::PathBuf;

use ariadne_core::{GoalStatus, SessionStatus, TaskStatus};
use ariadne_daemon::forge::poll::Mode;
use ariadne_store::{
    AgentSession, NewGoal, NewTask, NewTaskAgent, NewWorkflow, SessionFilter, Task,
};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::forge::{StubForgeCli, answer, opened_pull, stub_forge_cli};
use crate::common::{Harness, TIMEOUT, as_session, eventually, harness, sh, test_pin, with_forge};

const URL: &str = "https://github.com/acme/widgets/pull/1";

fn script(state: &str, head: &str, comments: &[Value], failed: &[Value]) -> Value {
    let mut pull = opened_pull(URL, head);
    pull["state"] = json!(state);
    let threads = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
        {"id": "T1", "isResolved": false, "comments": {"nodes": [{"databaseId": 101}]}}
    ]}}}}});
    json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["api", "user"], 0, "me"),
        answer(&["pr", "create"], 0, URL),
        answer(&["pr", "list"], 0, "[]"),
        answer(&["pr", "view"], 0, &pull.to_string()),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments/101/replies"],
            0,
            r#"{"id":201}"#
        ),
        answer(
            &["api", "repos/acme/widgets/pulls/1/comments"],
            0,
            &json!(comments).to_string()
        ),
        answer(&["api", "repos/acme/widgets/issues/1/comments"], 0, "[]"),
        answer(&["api", "repos/acme/widgets/pulls/1/reviews"], 0, "[]"),
        answer(&["api", "graphql"], 0, &threads.to_string()),
        answer(
            &["api", "repos/acme/widgets/commits/abc/check-runs"],
            0,
            &json!({"check_runs": failed}).to_string()
        ),
        answer(
            &["api", "repos/acme/widgets/compare/main...abc"],
            0,
            r#"{"behind_by":0}"#
        ),
    ])
}

struct RequestColumn {
    h: Harness,
    forge: StubForgeCli,
    task: Task,
    agent: AgentSession,
    id: String,
    path: PathBuf,
    remote: PathBuf,
    release: Option<PathBuf>,
}

async fn session_at(h: &Harness, task: &Task, step: &str) -> AgentSession {
    let agent = h
        .store
        .list_task_agents(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.step == step)
        .unwrap();
    eventually(
        TIMEOUT,
        "the column agent to receive its briefing",
        || async {
            h.store
                .list_sessions(SessionFilter {
                    task_id: Some(task.id.clone()),
                    ..Default::default()
                })
                .await
                .unwrap()
                .iter()
                .any(|s| s.task_agent_id.as_ref() == Some(&agent.id) && !h.prompts_to(s).is_empty())
        },
    )
    .await;
    h.store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .rev()
        .find(|s| s.task_agent_id.as_ref() == Some(&agent.id))
        .unwrap()
}

async fn request_column(hold_news: bool, deploy_after: bool) -> RequestColumn {
    let forge = stub_forge_cli(script("OPEN", "unused", &[], &[]));
    let h = harness().scheduler().forge_cli(&forge).await;
    let path = h.git_repo("widgets");
    let repo = h.repository(&path).await;
    with_forge(&h, &repo).await;
    sh(&path, "git push -q origin main");
    let workflow = if deploy_after {
        h.store.create_workflow(NewWorkflow {
            name: "pr-then-deploy".into(),
            document: "workflow pr-then-deploy\n develop[Develop]\n gate: committed\n review[Review]\n pr[Pull request]\n skills: pr-babysit\n gate: request-merged\n deploy[Deploy]\n".into(),
        }).await.unwrap();
        "pr-then-deploy"
    } else {
        "develop-review-pr"
    };
    let goal = h
        .store
        .create_goal(NewGoal {
            title: "Ship widgets".into(),
            description: "Build and open a request.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
            workflow: Some(workflow.into()),
        })
        .await
        .unwrap();
    let steps: &[&str] = if deploy_after {
        &["develop", "review", "pr", "deploy"]
    } else {
        &["develop", "review", "pr"]
    };
    let agents = steps
        .iter()
        .copied()
        .map(|step| NewTaskAgent::new(step, Vec::<String>::new(), test_pin()))
        .collect();
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Build widgets".into(),
            description: "Commit the widget change.".into(),
            agents,
            depends_on: vec![],
        })
        .await
        .unwrap();
    h.store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    h.notify(&task.id);
    let develop = session_at(&h, &task, "develop").await;
    let worktree = std::path::Path::new(develop.worktree_path.as_deref().unwrap());
    sh(
        worktree,
        "echo change > widget && git add widget && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add widgets'",
    );
    let step = |session: &AgentSession, reason: &str| {
        as_session(
            &format!("/v1/tasks/{}/step/complete", task.id),
            &session.id,
            json!({"reason": reason}),
        )
    };
    let _: Value = h
        .json(step(&develop, "The change is committed."), StatusCode::OK)
        .await;
    let review = session_at(&h, &task, "review").await;
    let release = hold_news.then(|| h.at("release-merge-news"));
    if let Some(release) = &release {
        let mut held = crate::common::acp::script();
        let first = held["prompts"][0].clone();
        held["prompts"] = json!([first, {"wait_for": release.display().to_string(),
            "updates": [], "stop_reason": "end_turn"}]);
        h.agent.reprogram(held);
    }
    let _: Value = h
        .json(step(&review, "The change passes review."), StatusCode::OK)
        .await;
    let agent = session_at(&h, &task, "pr").await;
    let remote = h.at("remote.git");
    sh(worktree, "git push -q origin HEAD");
    forge.reprogram(script("OPEN", &task.branch, &[], &[]));
    let open = || {
        as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &agent.id,
            json!({"title":"feat: add widgets", "body":"## Summary\nAdd widgets."}),
        )
    };
    let opened: Value = h.json(open(), StatusCode::OK).await;
    assert_eq!(opened["pr_url"], URL);
    let again: Value = h.json(open(), StatusCode::OK).await;
    assert_eq!(again["pr_url"], URL);
    let id = h
        .store
        .pull_request_of_task(&task.id)
        .await
        .unwrap()
        .unwrap()
        .id;
    h.state.forge_poll.set_mode(&repo.id, Mode::WakeOnly);
    eventually(TIMEOUT, "the request agent to become idle", async || {
        h.session_status(&agent).await == SessionStatus::Idle
    })
    .await;
    RequestColumn {
        h,
        forge,
        task,
        agent,
        id,
        path,
        remote,
        release,
    }
}

fn merge_request(request: &RequestColumn) -> String {
    let clone = request.h.at("forge-clone");
    sh(
        &request.h.at(""),
        &format!(
            "git clone -q '{}' '{}'",
            request.remote.display(),
            clone.display()
        ),
    );
    sh(
        &clone,
        &format!(
            "git checkout -q main && git -c user.name=Test -c user.email=test@test merge -q --no-ff origin/{} -m merged && git push -q origin HEAD:main",
            request.task.branch
        ),
    );
    sh(&clone, "git rev-parse HEAD")
}

#[tokio::test]
async fn the_pr_column_opens_the_request_once_and_keeps_it() {
    let request = request_column(false, false).await;
    let RequestColumn {
        h,
        forge,
        task,
        agent,
        id,
        path,
        ..
    } = &request;
    let create = forge
        .invocations()
        .into_iter()
        .filter(|call| call.args.starts_with(&["pr".into(), "create".into()]))
        .collect::<Vec<_>>();
    assert_eq!(create.len(), 1);
    assert_eq!(
        create[0].args,
        [
            "pr",
            "create",
            "--repo",
            "github.com/acme/widgets",
            "--head",
            &task.branch,
            "--base",
            "main",
            "--title",
            "feat: add widgets",
            "--body",
            "## Summary\nAdd widgets."
        ]
    );
    let dto: Value = h.get(&format!("/v1/pull-requests/{id}")).await;
    assert_eq!(dto["session_id"], agent.id);
    let before = h.prompts_to(agent).len();
    h.backdate(
        &["last_activity_at", "launched_at"],
        agent,
        ariadne_daemon::scheduler::QUIET_NUDGE_SECS + 5,
    )
    .await;
    h.sched
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::TaskChanged(
            task.id.clone(),
        ))
        .unwrap();
    h.flush_scheduler().await;
    assert_eq!(
        h.prompts_to(agent).len(),
        before,
        "an idle request agent waits on the forge"
    );
    let comment = json!({"id":101, "user":{"login":"alice", "type":"User"}, "body":"Please rename it.",
        "path":"widget", "line":1, "created_at":"2026-10-02T00:00:00Z"});
    let failed = json!({"name":"lint", "html_url":"https://ci.example/1", "conclusion":"failure", "status":"completed"});
    forge.reprogram(script(
        "OPEN",
        &task.branch,
        std::slice::from_ref(&comment),
        std::slice::from_ref(&failed),
    ));
    h.state.forge_poll.wake(&task.repo_id);
    eventually(
        TIMEOUT,
        "the comment and failed check to reach the agent",
        async || {
            h.prompts_to(agent)
                .iter()
                .skip(before)
                .any(|p| p.contains("rc-101") && p.contains("Check lint turned to failure"))
        },
    )
    .await;
    let told = h.prompts_to(agent).len();
    h.state.forge_poll.wake(&task.repo_id);
    h.flush_scheduler().await;
    assert_eq!(h.prompts_to(agent).len(), told);
    let reply: Value = h
        .json(
            as_session(
                &format!("/v1/pull-requests/{id}/comments/rc-101/reply"),
                &agent.id,
                json!({"body":"Renamed it."}),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(reply["author_login"], "me");
    assert!(forge.invocations().iter().any(|call| {
        call.args
            .iter()
            .any(|arg| arg.ends_with("/comments/101/replies"))
    }));
    let report = |ready| {
        as_session(
            &format!("/v1/pull-requests/{id}/report"),
            &agent.id,
            json!({"ready": ready, "head_sha": "a".repeat(40)}),
        )
    };
    let _: Value = h.json(report(true), StatusCode::OK).await;
    assert_eq!(h.attention(agent).await, None);
    let _: Value = h.json(report(false), StatusCode::OK).await;
    assert_eq!(h.attention(agent).await, None);
    let complete = || {
        as_session(
            &format!("/v1/tasks/{}/step/complete", task.id),
            &agent.id,
            json!({"reason":"The request merged."}),
        )
    };
    let refused = h.error(complete(), StatusCode::CONFLICT).await;
    assert_eq!(refused.error.code, "step_gate_failed");
    assert_eq!(h.status(&task.id).await, TaskStatus::InProgress);
    let merge = merge_request(&request);
    let mut merged = script("MERGED", &task.branch, &[comment], &[failed]);
    for entry in merged.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            let mut pull = opened_pull(URL, &task.branch);
            pull["state"] = json!("MERGED");
            pull["mergeCommit"] = json!({"oid": merge});
            entry["stdout"] = json!(pull.to_string());
        }
    }
    forge.reprogram(merged);
    let finished: Value = h.json(complete(), StatusCode::OK).await;
    assert_eq!(finished["status"], "finished");
    eventually(
        TIMEOUT,
        "the request and task work to be cleaned",
        async || {
            h.flush_scheduler().await;
            h.store.get_pull_request(id).await.is_err()
                && !std::path::Path::new(agent.worktree_path.as_deref().unwrap()).exists()
                && sh(path, &format!("git branch --list {}", task.branch)).is_empty()
        },
    )
    .await;
    for session in h
        .store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
    {
        assert!(!h.launcher.acp.is_running(&session.id));
    }
}

#[tokio::test]
async fn a_close_is_told_to_the_pr_agent_and_finishes_nothing() {
    let request = request_column(false, false).await;
    request
        .forge
        .reprogram(script("CLOSED", &request.task.branch, &[], &[]));
    request.h.state.forge_poll.wake(&request.task.repo_id);
    eventually(TIMEOUT, "the close to reach the agent", async || {
        request
            .h
            .prompts_to(&request.agent)
            .iter()
            .any(|p| p.contains("The request is now closed."))
    })
    .await;
    request.h.flush_scheduler().await;
    assert_eq!(
        request.h.status(&request.task.id).await,
        TaskStatus::InProgress
    );
}

#[tokio::test]
async fn a_quiet_pr_agent_is_finished_after_the_merge() {
    let request = request_column(true, false).await;
    let release = request.release.as_ref().unwrap();
    let merge = merge_request(&request);
    let mut merged = script("MERGED", &request.task.branch, &[], &[]);
    for entry in merged.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            let mut pull = opened_pull(URL, &request.task.branch);
            pull["state"] = json!("MERGED");
            pull["mergeCommit"] = json!({"oid": merge});
            entry["stdout"] = json!(pull.to_string());
        }
    }
    request.forge.reprogram(merged);
    request.h.state.forge_poll.wake(&request.task.repo_id);
    eventually(TIMEOUT, "the merge to reach the agent", async || {
        release.with_extension("reached").exists()
            && request
                .h
                .prompts_to(&request.agent)
                .iter()
                .any(|p| p.contains("The request is now merged."))
    })
    .await;
    assert_eq!(
        request.h.status(&request.task.id).await,
        TaskStatus::InProgress
    );
    let database =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", request.h.at("test.db").display()))
            .await
            .unwrap();
    let earlier = (chrono::Utc::now()
        - chrono::Duration::seconds(ariadne_daemon::scheduler::QUIET_NUDGE_SECS + 5))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query("UPDATE pull_requests SET news_told_at = ? WHERE id = ?")
        .bind(earlier)
        .bind(&request.id)
        .execute(&database)
        .await
        .unwrap();
    database.close().await;
    request
        .h
        .sched
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::PullRequestChanged(
            request.id.clone(),
        ))
        .unwrap();
    eventually(
        TIMEOUT,
        "the daemon to finish the merged task",
        async || {
            request.h.flush_scheduler().await;
            request.h.status(&request.task.id).await == TaskStatus::Finished
        },
    )
    .await;
    assert_eq!(
        request
            .h
            .store
            .get_task(&request.task.id)
            .await
            .unwrap()
            .merge_commit
            .as_deref(),
        Some(merge.as_str())
    );
}

#[tokio::test]
async fn a_quiet_pr_agent_advances_to_the_column_after_the_merge() {
    let request = request_column(true, true).await;
    let release = request.release.as_ref().unwrap();
    let merge = merge_request(&request);
    let mut merged = script("MERGED", &request.task.branch, &[], &[]);
    for entry in merged.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            let mut pull = opened_pull(URL, &request.task.branch);
            pull["state"] = json!("MERGED");
            pull["mergeCommit"] = json!({"oid": merge});
            entry["stdout"] = json!(pull.to_string());
        }
    }
    request.forge.reprogram(merged);
    request.h.state.forge_poll.wake(&request.task.repo_id);
    eventually(
        TIMEOUT,
        "the merge news to reach the request agent",
        async || {
            release.with_extension("reached").exists()
                && request
                    .h
                    .prompts_to(&request.agent)
                    .iter()
                    .any(|prompt| prompt.contains("The request is now merged."))
        },
    )
    .await;
    let database =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", request.h.at("test.db").display()))
            .await
            .unwrap();
    let earlier = (chrono::Utc::now()
        - chrono::Duration::seconds(ariadne_daemon::scheduler::QUIET_NUDGE_SECS + 5))
    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    sqlx::query("UPDATE pull_requests SET news_told_at = ? WHERE id = ?")
        .bind(earlier)
        .bind(&request.id)
        .execute(&database)
        .await
        .unwrap();
    database.close().await;
    request
        .h
        .sched
        .as_ref()
        .unwrap()
        .send(ariadne_daemon::scheduler::SchedEvent::PullRequestChanged(
            request.id.clone(),
        ))
        .unwrap();
    eventually(TIMEOUT, "the daemon to advance to deployment", async || {
        request.h.flush_scheduler().await;
        let task = request.h.store.get_task(&request.task.id).await.unwrap();
        task.status() == TaskStatus::InProgress && task.step.as_deref() == Some("deploy")
    })
    .await;
    let deploy = session_at(&request.h, &request.task, "deploy").await;
    assert_eq!(deploy.worktree_path, request.agent.worktree_path);
    assert!(std::path::Path::new(deploy.worktree_path.as_deref().unwrap()).exists());
    assert!(request.h.store.get_pull_request(&request.id).await.is_ok());
    assert!(
        !sh(
            &request.path,
            &format!("git branch --list {}", request.task.branch)
        )
        .is_empty()
    );
    std::fs::write(release, "").unwrap();
    let finished: Value = request
        .h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", request.task.id),
                &deploy.id,
                json!({"reason": "Deployment finished."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["merge_commit"], merge);
}

/// The request column is not always the task's last: the agent completes
/// it itself once the forge says merged, moving the task on to the column
/// after it rather than finishing the task there. The merge commit the
/// gate read off the forge must survive that move, since the column it
/// moves to has none of its own to give the task when it finally finishes.
#[tokio::test]
async fn completing_the_request_column_by_hand_carries_its_merge_commit_onward() {
    let request = request_column(true, true).await;
    let merge = merge_request(&request);
    let mut merged = script("MERGED", &request.task.branch, &[], &[]);
    for entry in merged.as_array_mut().unwrap() {
        if entry["args"] == json!(["pr", "view"]) {
            let mut pull = opened_pull(URL, &request.task.branch);
            pull["state"] = json!("MERGED");
            pull["mergeCommit"] = json!({"oid": merge});
            entry["stdout"] = json!(pull.to_string());
        }
    }
    request.forge.reprogram(merged);

    let moved: Value = request
        .h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", request.task.id),
                &request.agent.id,
                json!({"reason": "The request merged."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved["step"], "deploy");

    let deploy = session_at(&request.h, &request.task, "deploy").await;
    let finished: Value = request
        .h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", request.task.id),
                &deploy.id,
                json!({"reason": "Deployment finished."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["merge_commit"], merge);
}
