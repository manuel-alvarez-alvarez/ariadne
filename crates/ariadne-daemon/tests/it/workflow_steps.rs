use crate::common::{Harness, TIMEOUT, as_session, eventually, sh, test_pin, with_forge};
use ariadne_core::{Actor, GoalStatus, Seat, TaskStatus};
use ariadne_store::{AgentSession, NewGoal, NewTask, NewTaskAgent, SessionFilter, Task};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::common::{harness, post_json};

/// A goal created with no workflow runs on its repository's default and
/// exposes that workflow's columns; one that names a workflow snapshots it.
#[tokio::test]
async fn a_goal_takes_the_repository_default_and_snapshots_its_workflow() {
    let h = harness().await;
    let (defaulted, repo) = h.goal().await;
    let defaulted: ariadne_api::goals::GoalDto =
        h.get(&format!("/v1/goals/{}", defaulted.id)).await;
    assert_eq!(defaulted.workflow, repo.default_workflow);
    assert_eq!(defaulted.workflow, "develop-review-merge");
    assert_eq!(
        defaulted
            .steps
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["develop", "review", "merge"]
    );
    assert!(defaulted.usage.agents.is_empty());
    let goal: Value = h
        .json(
            post_json(
                "/v1/goals",
                json!({
                    "title": "Build a change", "description": "Run its workflow.",
                    "repository_ids": [repo.id], "model": "stub:test-model",
                    "workflow": "develop-review-pr"
                }),
            ),
            StatusCode::CREATED,
        )
        .await;
    assert_eq!(goal["workflow"], "develop-review-pr");
    assert_eq!(
        goal["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["develop", "review", "pr"]
    );
}

async fn stepped(h: &Harness) -> Task {
    let path = h.git_repo("steps-repo");
    let repo = h.repository(&path).await;
    let goal = h
        .store
        .create_goal(NewGoal {
            title: "Run the columns".into(),
            description: "Build and land a change.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
            workflow: Some("develop-review-merge".into()),
        })
        .await
        .unwrap();
    let agents = ["develop", "review", "merge"]
        .into_iter()
        .map(|step| {
            let mut pin = test_pin();
            pin.model = format!("stub:{step}-model");
            NewTaskAgent::new(step, Vec::<String>::new(), pin)
        })
        .collect();
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "Build the feature".into(),
            description: "Commit the change.".into(),
            agents,
            depends_on: vec![],
        })
        .await
        .unwrap();
    h.store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    if h.sched.is_some() {
        h.notify(&task.id);
    }
    task
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

async fn call(
    h: &Harness,
    task: &Task,
    session: &AgentSession,
    action: &str,
    reason: &str,
) -> Value {
    h.json(
        as_session(
            &format!("/v1/tasks/{}/step/{action}", task.id),
            &session.id,
            json!({"reason": reason}),
        ),
        StatusCode::OK,
    )
    .await
}

#[tokio::test]
async fn a_task_walks_develop_review_merge_with_one_agent_per_column() {
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    let worktree = develop.worktree_path.as_deref().unwrap();
    assert_eq!(
        sh(std::path::Path::new(worktree), "git branch --show-current").trim(),
        task.branch
    );
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("develop")
    );
    assert_eq!(
        h.store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &develop.id,
                json!({"reason":"Ready."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    sh(
        std::path::Path::new(worktree),
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add the feature'",
    );
    let dirty = std::path::Path::new(worktree).join("uncommitted");
    std::fs::write(&dirty, "unfinished change").unwrap();
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &develop.id,
                json!({"reason":"Ready."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("develop")
    );
    std::fs::remove_file(dirty).unwrap();
    assert_eq!(
        call(&h, &task, &develop, "complete", "The feature is committed.").await["step"],
        "review"
    );
    let review = session_at(&h, &task, "review").await;
    assert_eq!(review.worktree_path, develop.worktree_path);
    assert!(h.prompted(&review).contains("The feature is committed."));
    assert_eq!(
        call(&h, &task, &review, "fail", "Fix the edge case.").await["step"],
        "develop"
    );
    eventually(TIMEOUT, "the return feedback", || async {
        h.prompted(&develop).contains("Fix the edge case.")
    })
    .await;
    assert!(h.prompted(&develop).contains("Direction: back"));
    call(&h, &task, &develop, "complete", "The edge case is fixed.").await;
    eventually(TIMEOUT, "the second review briefing", || async {
        h.prompted(&review).contains("The edge case is fixed.")
    })
    .await;
    assert!(h.prompted(&review).contains("Direction: forward"));
    call(&h, &task, &review, "complete", "The change passes review.").await;
    let merge = session_at(&h, &task, "merge").await;
    assert_eq!(merge.worktree_path, develop.worktree_path);
    for (session, step, skill) in [
        (&develop, "develop", "coding"),
        (&review, "review", "code-review"),
        (&merge, "merge", "merge"),
    ] {
        assert_eq!(session.model, format!("stub:{step}-model"));
        let launch = h.launch_file(&session.id).unwrap();
        assert_eq!(launch.model, format!("{step}-model"));
        let document = h
            .launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        assert!(
            launch
                .system_prompt
                .contains(&document.display().to_string())
        );
        assert!(!std::fs::read_to_string(document).unwrap().is_empty());
    }
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &merge.id,
                json!({"reason":"Landed."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    let repo = h.store.get_repository(&task.repo_id).await.unwrap();
    let unmerged_sha = sh(std::path::Path::new(worktree), "git rev-parse HEAD");
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &merge.id,
                json!({"reason":"Landed.", "merge_commit":unmerged_sha.trim()}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("merge")
    );
    sh(
        std::path::Path::new(&repo.path),
        &format!("git merge --ff-only {}", task.branch),
    );
    let sha = sh(std::path::Path::new(&repo.path), "git rev-parse HEAD");
    let finished: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &merge.id,
                json!({"reason":"The change is on the base.","merge_commit":sha.trim()}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(finished["status"], "finished");
    let transitions = h.store.list_task_transitions(&task.id).await.unwrap();
    let moves: Vec<_> = transitions
        .iter()
        .filter(|t| t.actor == "agent")
        .map(|t| {
            (
                t.from_step.as_deref(),
                t.to_step.as_deref(),
                t.reason.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        moves,
        [
            (
                Some("develop"),
                Some("review"),
                Some("The feature is committed.")
            ),
            (Some("review"), Some("develop"), Some("Fix the edge case.")),
            (
                Some("develop"),
                Some("review"),
                Some("The edge case is fixed.")
            ),
            (
                Some("review"),
                Some("merge"),
                Some("The change passes review.")
            ),
            (
                Some("merge"),
                Some("merge"),
                Some("The change is on the base.")
            ),
        ]
    );
    eventually(TIMEOUT, "all task agents to stop", || async {
        [&develop, &review, &merge]
            .iter()
            .all(|s| !h.launcher.acp.is_running(&s.id))
    })
    .await;
    assert_eq!(
        h.store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        3
    );
    let env = h.launch_file(&develop.id).unwrap().mcp_servers[0]
        .env
        .clone();
    assert!(
        env.iter()
            .any(|v| v.name == "ARIADNE_SEAT" && v.value == "agent")
    );
}

#[tokio::test]
async fn a_first_column_failure_retries_on_develop_with_the_same_session() {
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    let failed = call(&h, &task, &develop, "fail", "The input is missing.").await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["reason"], "The input is missing.");
    eventually(TIMEOUT, "the failed agent to stop", || async {
        !h.launcher.acp.is_running(&develop.id)
    })
    .await;
    let _: Value = h
        .json(
            post_json(&format!("/v1/tasks/{}/retry", task.id), json!({})),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the retry briefing", || async {
        h.prompted(&develop).contains("Direction: retry")
    })
    .await;
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("develop")
    );
    assert_eq!(
        h.store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A retry puts a task back on its first column, so every column has to be
/// staffed before it: a failed task with a column nobody staffs is refused by
/// that column's name, and the task stays failed until `update_task` staffs
/// it, after which the same retry is taken.
#[tokio::test]
async fn a_retry_refuses_a_task_with_an_unstaffed_column_by_name() {
    let h = harness().await;
    let task = stepped(&h).await;
    h.advance(&task, TaskStatus::InProgress).await;
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Failed,
            Actor::Agent,
            Some("the input is missing"),
            None,
        )
        .await
        .unwrap();
    // The merge column has no agent, the way a database written before
    // workflows leaves a task of a request goal with no agent on `pr`
    // (0023). No edit produces that on an active goal, so the row goes the
    // way the migration left it: straight from the table.
    h.unstaff_column(&task, "merge").await;
    let unstaffed: Value = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert_eq!(unstaffed["agents"].as_array().unwrap().len(), 2);

    let refused = h
        .error(
            post_json(&format!("/v1/tasks/{}/retry", task.id), json!({})),
            StatusCode::CONFLICT,
        )
        .await;
    assert!(
        refused.error.message.contains("no agent on column merge"),
        "{}",
        refused.error.message
    );
    assert!(
        refused.error.message.contains("update_task"),
        "{}",
        refused.error.message
    );
    assert_eq!(h.status(&task.id).await, TaskStatus::Failed);

    let _: Value = h
        .json(
            crate::common::patch_json(
                &format!("/v1/tasks/{}", task.id),
                json!({"agents": [
                    {"step": "develop", "model": "stub:develop-model"},
                    {"step": "review", "model": "stub:review-model"},
                    {"step": "merge", "model": "stub:merge-model"},
                ]}),
            ),
            StatusCode::OK,
        )
        .await;
    let retried: Value = h
        .json(
            post_json(&format!("/v1/tasks/{}/retry", task.id), json!({})),
            StatusCode::OK,
        )
        .await;
    assert_eq!(retried["status"], "ready");
}

/// A task that can start needs an agent on every column. Once its goal runs,
/// a create or an edit that leaves a column unstaffed is refused by name; a
/// task that reaches the scheduler unstaffed all the same — one a database
/// from before workflows carried — fails before it starts, naming the column,
/// and the retry takes it once the column is staffed.
#[tokio::test]
async fn a_runnable_task_with_an_unstaffed_column_fails_before_it_starts() {
    let h = harness().scheduler().await;
    let path = h.git_repo("steps-repo");
    let repo = h.repository(&path).await;
    let goal = h
        .store
        .create_goal(NewGoal {
            title: "Run the columns".into(),
            description: "Build and land a change.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
            workflow: Some("develop-review-merge".into()),
        })
        .await
        .unwrap();
    // Written while the goal was planned, with its first column alone.
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id.clone(),
            title: "Build the feature".into(),
            description: "Commit the change.".into(),
            agents: vec![NewTaskAgent::new(
                "develop",
                Vec::<String>::new(),
                test_pin(),
            )],
            depends_on: vec![],
        })
        .await
        .unwrap();
    h.store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();

    // On the active goal, a create or an edit that leaves a column out is
    // refused by name before anything is written.
    let refused = h
        .error(
            post_json(
                &format!("/v1/goals/{}/tasks", goal.id),
                json!({"title": "Another", "description": "", "agents": [
                    {"step": "review", "model": "stub:test-model"}
                ]}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        refused.error.message.contains("none staffs develop, merge"),
        "{}",
        refused.error.message
    );
    let refused = h
        .error(
            crate::common::patch_json(
                &format!("/v1/tasks/{}", task.id),
                json!({"agents": [{"step": "develop", "model": "stub:test-model"}]}),
            ),
            StatusCode::BAD_REQUEST,
        )
        .await;
    assert!(
        refused.error.message.contains("none staffs review, merge"),
        "{}",
        refused.error.message
    );

    // The scheduler finds the task runnable and unstaffed: it fails naming
    // the columns, and spends no launch on it.
    h.notify(&task.id);
    eventually(TIMEOUT, "the unstaffed task to fail", || async {
        h.status(&task.id).await == TaskStatus::Failed
    })
    .await;
    let failed: Value = h.get(&format!("/v1/tasks/{}", task.id)).await;
    assert!(
        failed["reason"]
            .as_str()
            .is_some_and(|r| r.contains("no agent on column review, merge")),
        "{}",
        failed["reason"]
    );
    assert!(
        h.sessions_of(&task.id).await.is_empty(),
        "no agent was started"
    );

    // Staffed whole, the retry starts the task on its first column.
    let _: Value = h
        .json(
            crate::common::patch_json(
                &format!("/v1/tasks/{}", task.id),
                json!({"agents": [
                    {"step": "develop", "model": "stub:test-model"},
                    {"step": "review", "model": "stub:test-model"},
                    {"step": "merge", "model": "stub:test-model"},
                ]}),
            ),
            StatusCode::OK,
        )
        .await;
    let _: Value = h
        .json(
            post_json(&format!("/v1/tasks/{}/retry", task.id), json!({})),
            StatusCode::OK,
        )
        .await;
    let develop = session_at(&h, &task, "develop").await;
    assert!(h.launcher.acp.is_running(&develop.id));
}

#[tokio::test]
async fn another_column_and_the_orchestrator_cannot_move_a_step() {
    let h = harness().await;
    let task = stepped(&h).await;
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    h.store.start_first_step(&task.id).await.unwrap();
    let goal = h.store.get_goal(&task.goal_id).await.unwrap();
    let review = h
        .store
        .list_task_agents(&task.id)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.step == "review")
        .unwrap();
    let other = h.session(&goal, Some(&task), Seat::Agent, &review.id).await;
    let orchestrator = h.orchestrator_session(&goal).await;
    for session in [&other, &orchestrator] {
        for action in ["complete", "fail"] {
            h.error(
                as_session(
                    &format!("/v1/tasks/{}/step/{action}", task.id),
                    &session.id,
                    json!({"reason":"Move."}),
                ),
                StatusCode::FORBIDDEN,
            )
            .await;
        }
    }
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("develop")
    );
}

#[tokio::test]
async fn only_the_current_column_is_nudged_and_idle_columns_raise_no_attention() {
    use ariadne_daemon::scheduler::QUIET_NUDGE_SECS;
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    sh(
        std::path::Path::new(develop.worktree_path.as_deref().unwrap()),
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add change'",
    );
    call(&h, &task, &develop, "complete", "Review this change.").await;
    let review = session_at(&h, &task, "review").await;
    eventually(TIMEOUT, "both agents to finish their turns", || async {
        h.store.get_session(&develop.id).await.unwrap().status()
            == ariadne_core::SessionStatus::Idle
            && h.store.get_session(&review.id).await.unwrap().status()
                == ariadne_core::SessionStatus::Idle
    })
    .await;
    let develop_prompts = h.prompts_to(&develop).len();
    let review_prompts = h.prompts_to(&review).len();
    h.backdate(
        &["last_activity_at", "launched_at"],
        &develop,
        ariadne_daemon::scheduler::QUIET_RELAUNCH_SECS + 5,
    )
    .await;
    h.backdate(
        &["last_activity_at", "launched_at"],
        &review,
        QUIET_NUDGE_SECS + 5,
    )
    .await;
    h.flush_scheduler().await;
    eventually(TIMEOUT, "the review nudge", || async {
        h.prompts_to(&review).len() > review_prompts
    })
    .await;
    assert_eq!(h.prompts_to(&develop).len(), develop_prompts);
    assert!(
        h.store
            .get_session(&develop.id)
            .await
            .unwrap()
            .attention_reason()
            .is_none()
    );
    assert!(
        h.prompted(&review)
            .contains("Continue Build the feature at Review.")
    );
    assert!(!ariadne_daemon::attention::work_is_active(&h.store, &develop).await);
    let _: Value = h
        .json(
            post_json(
                &format!("/v1/tasks/{}/cancel", task.id),
                json!({"reason":"Stop the task."}),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the cancelled task agents to stop", || async {
        !h.launcher.acp.is_running(&develop.id) && !h.launcher.acp.is_running(&review.id)
    })
    .await;
}

#[tokio::test]
async fn a_step_briefing_survives_a_closed_prompt_channel() {
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    sh(
        std::path::Path::new(develop.worktree_path.as_deref().unwrap()),
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add change'",
    );
    call(&h, &task, &develop, "complete", "The change is ready.").await;
    let review = session_at(&h, &task, "review").await;
    h.launcher.acp.close_prompt_channel_for_test(&develop.id);
    call(&h, &task, &review, "fail", "Repair this boundary.").await;
    h.flush_scheduler().await;
    let transition = h
        .store
        .list_task_transitions(&task.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert!(!h.store.step_briefed(&transition.id).await.unwrap());
    h.launcher.kill_session(&develop.id).await.unwrap();
    h.flush_scheduler().await;
    eventually(TIMEOUT, "the undelivered return briefing", || async {
        h.prompted(&develop).contains("Repair this boundary.")
    })
    .await;
    h.flush_scheduler().await;
    assert_eq!(
        h.prompts_to(&develop)
            .iter()
            .filter(|p| p.contains("Repair this boundary."))
            .count(),
        1
    );
}

/// A pass that read the task before its column completed sees the next
/// column's entry as the newest one. That entry's briefing is the next
/// column's agent's, and the agent that just left the column is handed
/// nothing for it: briefed there, it would take the next column's one
/// briefing with it.
#[tokio::test]
async fn a_pass_behind_a_completion_hands_the_next_entry_to_nobody() {
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    sh(
        std::path::Path::new(develop.worktree_path.as_deref().unwrap()),
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add change'",
    );
    // The reviewer holds its first turn open, so whatever a pass hands it
    // waits in its queue until the move is whole.
    let release = h.dir.path().join("review-turn");
    let mut script = crate::common::acp::script();
    script["stored_sessions"] = json!(["uuid-1234", "stub-session"]);
    script["prompts"] = json!([{"wait_for": release.display().to_string(), "updates": []}]);
    h.agent.reprogram(script);
    call(&h, &task, &develop, "complete", "The change is ready.").await;
    let review = session_at(&h, &task, "review").await;
    eventually(TIMEOUT, "the reviewer inside its turn", || async {
        release.with_extension("reached").exists()
    })
    .await;

    h.entry_ahead_of_its_task(&task, "review", "merge").await;
    h.notify(&task.id);
    h.flush_scheduler().await;
    h.task_row_on(&task, "merge").await;
    std::fs::write(&release, "go").unwrap();
    h.notify(&task.id);
    h.flush_scheduler().await;

    let merge = session_at(&h, &task, "merge").await;
    assert!(h.prompted(&merge).contains("The column is done."));
    assert_eq!(
        h.prompts_to(&review).len(),
        1,
        "{:?}",
        h.prompts_to(&review)
    );
}

async fn gated_task(h: &Harness, gate: &str) -> (Task, AgentSession) {
    let path = h.git_repo("gate-repo");
    let repo = h.repository(&path).await;
    h.store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "gated".into(),
            document: format!("workflow gated\n build[Build]\n gate: {gate}\n"),
        })
        .await
        .unwrap();
    let goal = h
        .store
        .create_goal(NewGoal {
            workflow: Some("gated".into()),
            title: "Gate the result".into(),
            description: "Check the branch.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
        })
        .await
        .unwrap();
    let agent = NewTaskAgent::new("build", Vec::<String>::new(), test_pin());
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "Gate".into(),
            description: "Do the work.".into(),
            agents: vec![agent],
            depends_on: vec![],
        })
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let task = h.store.start_first_step(&task.id).await.unwrap();
    let worktree = h.at("gate-worktree");
    h.launcher
        .git
        .add_worktree(&path, &worktree, &task.branch, "main")
        .await
        .unwrap();
    h.store
        .set_task_worktree(&task.id, Some(worktree.to_str().unwrap()))
        .await
        .unwrap();
    let agent = h.store.list_task_agents(&task.id).await.unwrap().remove(0);
    let session = h
        .store
        .create_session(ariadne_store::NewSession {
            goal_id: Some(goal.id),
            task_id: Some(task.id.clone()),
            seat: Some(Seat::Agent),
            task_agent_id: Some(agent.id),
            model: test_pin().model,
            effort: None,
            worktree_path: Some(worktree.to_str().unwrap().into()),
            pull_request_id: None,
        })
        .await
        .unwrap();
    (task, session)
}

#[tokio::test]
async fn the_push_gate_requires_the_current_tip_on_the_remote() {
    let h = harness().await;
    let (task, session) = gated_task(&h, "pushed").await;
    let repo = h.store.get_repository(&task.repo_id).await.unwrap();
    with_forge(&h, &repo).await;
    let uri = format!("/v1/tasks/{}/step/complete", task.id);
    let refused = h
        .error(
            as_session(&uri, &session.id, json!({"reason":"Pushed."})),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    sh(
        std::path::Path::new(session.worktree_path.as_deref().unwrap()),
        "git push -q origin HEAD",
    );
    assert_eq!(
        call(&h, &task, &session, "complete", "The tip is on the remote.").await["status"],
        "finished"
    );
}

#[tokio::test]
async fn a_step_agent_opens_its_request_and_completion_reads_the_forge_now() {
    use crate::common::forge::{answer, opened_pull, stub_forge_cli};
    let url = "https://github.com/acme/widgets/pull/1";
    let forge = stub_forge_cli(json!([]));
    let h = harness().forge_cli(&forge).await;
    let (task, session) = gated_task(&h, "request-merged").await;
    let repo = h.store.get_repository(&task.repo_id).await.unwrap();
    with_forge(&h, &repo).await;
    sh(
        std::path::Path::new(session.worktree_path.as_deref().unwrap()),
        "git push -q origin HEAD",
    );
    let mut pull = opened_pull(url, &task.branch);
    forge.reprogram(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["pr", "create"], 0, url),
        answer(&["pr", "view"], 0, &pull.to_string())
    ]));
    let opened: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/pull-request", task.id),
                &session.id,
                json!({"title":"feat: add a change", "body":"Build the change."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(opened["pr_url"], url);
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &session.id,
                json!({"reason":"Request ready."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    pull["state"] = json!("MERGED");
    forge.reprogram(json!([answer(&["pr", "view"], 0, &pull.to_string())]));
    assert_eq!(
        call(&h, &task, &session, "complete", "The request merged.").await["status"],
        "finished"
    );
    assert_eq!(
        forge
            .invocations()
            .iter()
            .filter(|i| i.args.starts_with(&["pr".into(), "view".into()]))
            .count(),
        3
    );
}

/// Completing the request column with a reason alone — what `call` above
/// sends, and what the `pr-babysit` skill sends — still records the forge's
/// own merge commit rather than none: the diff route reads it once cleanup
/// has deleted the branch.
#[tokio::test]
async fn completing_the_request_column_without_a_merge_commit_records_the_forges() {
    use crate::common::forge::{answer, opened_pull, stub_forge_cli};
    let url = "https://github.com/acme/widgets/pull/1";
    let forge = stub_forge_cli(json!([]));
    let h = harness().forge_cli(&forge).await;
    let (task, session) = gated_task(&h, "request-merged").await;
    let repo = h.store.get_repository(&task.repo_id).await.unwrap();
    with_forge(&h, &repo).await;
    sh(
        std::path::Path::new(session.worktree_path.as_deref().unwrap()),
        "git push -q origin HEAD",
    );
    let mut pull = opened_pull(url, &task.branch);
    forge.reprogram(json!([
        answer(&["auth", "status"], 0, ""),
        answer(&["pr", "create"], 0, url),
        answer(&["pr", "view"], 0, &pull.to_string())
    ]));
    h.json::<Value>(
        as_session(
            &format!("/v1/tasks/{}/pull-request", task.id),
            &session.id,
            json!({"title":"feat: add a change", "body":"Build the change."}),
        ),
        StatusCode::OK,
    )
    .await;
    pull["state"] = json!("MERGED");
    pull["mergeCommit"] = json!({"oid": "merge-sha-123"});
    forge.reprogram(json!([answer(&["pr", "view"], 0, &pull.to_string())]));

    let finished = call(&h, &task, &session, "complete", "The request merged.").await;
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["merge_commit"], "merge-sha-123");
}

/// A workflow may put its `merged` gate before another column, same as a
/// `request-merged` one can: `merge[Merge] gate: merged` then
/// `deploy[Deploy]`, with no gate of its own. Both columns' sessions are
/// made up front, with no scheduler running to spawn the second one: the
/// column-agent check `complete` makes only matches a session to its
/// agent's own column, not to a live process.
async fn merge_then_deploy(h: &Harness) -> (Task, AgentSession, AgentSession) {
    let path = h.git_repo("merge-deploy-repo");
    let repo = h.repository(&path).await;
    h.store
        .create_workflow(ariadne_store::NewWorkflow {
            name: "merge-then-deploy".into(),
            document:
                "workflow merge-then-deploy\n merge[Merge]\n  gate: merged\n deploy[Deploy]\n"
                    .into(),
        })
        .await
        .unwrap();
    let goal = h
        .store
        .create_goal(NewGoal {
            workflow: Some("merge-then-deploy".into()),
            title: "Ship it".into(),
            description: "Land the change.".into(),
            issue_url: None,
            repository_ids: vec![repo.id.clone()],
            pin: test_pin(),
        })
        .await
        .unwrap();
    let agents = ["merge", "deploy"]
        .map(|step| NewTaskAgent::new(step, Vec::<String>::new(), test_pin()))
        .to_vec();
    let task = h
        .store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "Ship it".into(),
            description: "Do the work.".into(),
            agents,
            depends_on: vec![],
        })
        .await
        .unwrap();
    h.store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();
    let task = h.store.start_first_step(&task.id).await.unwrap();
    let worktree = h.at("merge-deploy-worktree");
    h.launcher
        .git
        .add_worktree(&path, &worktree, &task.branch, "main")
        .await
        .unwrap();
    h.store
        .set_task_worktree(&task.id, Some(worktree.to_str().unwrap()))
        .await
        .unwrap();
    let agents = h.store.list_task_agents(&task.id).await.unwrap();
    let merge_agent = agents.iter().find(|a| a.step == "merge").unwrap().clone();
    let deploy_agent = agents.iter().find(|a| a.step == "deploy").unwrap().clone();
    let session_on = |agent_id: String| ariadne_store::NewSession {
        goal_id: Some(goal.id.clone()),
        task_id: Some(task.id.clone()),
        seat: Some(Seat::Agent),
        task_agent_id: Some(agent_id),
        model: test_pin().model,
        effort: None,
        worktree_path: Some(worktree.to_str().unwrap().into()),
        pull_request_id: None,
    };
    let merge = h
        .store
        .create_session(session_on(merge_agent.id))
        .await
        .unwrap();
    let deploy = h
        .store
        .create_session(session_on(deploy_agent.id))
        .await
        .unwrap();
    sh(
        &worktree,
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add the feature'",
    );
    (task, merge, deploy)
}

/// Completing a `merged` gate before the last column still carries the
/// verified merge commit onward: the move to the column after it must not
/// drop it, since that later column has none of its own to give the task
/// when it finishes, and cleanup leaves no branch for the diff route to
/// read instead.
#[tokio::test]
async fn completing_a_merge_column_before_the_last_carries_its_merge_commit_onward() {
    let h = harness().await;
    let (task, merge, deploy) = merge_then_deploy(&h).await;
    let repo = h.store.get_repository(&task.repo_id).await.unwrap();
    sh(
        std::path::Path::new(&repo.path),
        &format!("git merge --ff-only {}", task.branch),
    );
    let sha = sh(std::path::Path::new(&repo.path), "git rev-parse HEAD");
    let sha = sha.trim();

    let moved: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &merge.id,
                json!({"reason": "The change is on the base.", "merge_commit": sha}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(moved["step"], "deploy");

    let finished: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &deploy.id,
                json!({"reason": "Deployment finished."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["merge_commit"], sha);
}

#[tokio::test]
async fn any_column_can_fail_the_task_and_usage_and_facts_name_the_column() {
    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    sh(
        std::path::Path::new(develop.worktree_path.as_deref().unwrap()),
        "echo change > feature && git add feature && git -c user.name=Test -c user.email=test@test commit -qm 'feat: add change'",
    );
    call(&h, &task, &develop, "complete", "Review this change.").await;
    let review = session_at(&h, &task, "review").await;
    h.store
        .upsert_session_usage(
            &review.id,
            "acp",
            ariadne_core::TokenUsage {
                input_tokens: 12,
                cached_input_tokens: 3,
                output_tokens: 4,
            },
        )
        .await
        .unwrap();
    let read: Value = h.get(&format!("/v1/tasks/{}", task.id)).await;
    let usage = read["usage"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["step"] == "review")
        .unwrap();
    assert_eq!(usage["usage"]["input_tokens"], 12);
    let goal: Value = h.get(&format!("/v1/goals/{}", task.goal_id)).await;
    assert!(goal["usage"]["agents"].as_array().unwrap().contains(usage));
    let fact = ariadne_daemon::stats::session_fact(&h.store, &review.id, "test", json!({}))
        .await
        .unwrap();
    assert_eq!(fact.seat.as_deref(), Some("agent"));
    assert_eq!(fact.data["step"], "review");
    let failed: Value = h
        .json(
            as_session(
                &format!("/v1/tasks/{}/transitions", task.id),
                &develop.id,
                json!({"to":"failed", "reason":"The task cannot proceed."}),
            ),
            StatusCode::OK,
        )
        .await;
    assert_eq!(failed["reason"], "The task cannot proceed.");
    eventually(TIMEOUT, "all failed task agents to stop", || async {
        !h.launcher.acp.is_running(&develop.id) && !h.launcher.acp.is_running(&review.id)
    })
    .await;
}

#[tokio::test]
async fn openapi_describes_the_step_routes_and_workflow_fields() {
    let h = harness().await;
    let doc: Value = h.get("/api-docs/openapi.json").await;
    for (action, body) in [
        ("complete", "CompleteStepRequest"),
        ("fail", "FailStepRequest"),
    ] {
        let route = &doc["paths"][format!("/v1/tasks/{{id}}/step/{action}")]["post"];
        assert_eq!(
            route["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            format!("#/components/schemas/{body}")
        );
        for code in ["200", "403", "409"] {
            assert!(route["responses"][code].is_object());
        }
    }
    for (schema, fields) in [
        ("GoalDto", vec!["workflow", "steps"]),
        ("CreateGoalRequest", vec!["workflow"]),
        ("RepositoryDto", vec!["default_workflow"]),
        ("CreateRepositoryRequest", vec!["default_workflow"]),
        ("UpdateRepositoryRequest", vec!["default_workflow"]),
        ("TaskDto", vec!["step"]),
        ("TaskAgentDto", vec!["step"]),
        ("AgentAssignment", vec!["step"]),
        ("UpdateTaskRequest", vec!["agents"]),
        ("TaskTransitionDto", vec!["from_step", "to_step"]),
        ("GoalUsageDto", vec!["agents"]),
        ("TaskUsageDto", vec!["agents"]),
        ("AgentUsageDto", vec!["step"]),
        ("CompleteStepRequest", vec!["reason", "merge_commit"]),
        ("FailStepRequest", vec!["reason"]),
    ] {
        for field in fields {
            assert!(
                doc["components"]["schemas"][schema]["properties"][field].is_object(),
                "{schema}.{field}"
            );
        }
    }
    assert!(
        doc["components"]["schemas"]["Seat"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("agent"))
    );
}

#[tokio::test]
async fn the_orchestrator_reads_the_workflow_and_agents_load_their_own_skills() {
    let h = harness().await;
    let task = stepped(&h).await;
    let agents = h
        .store
        .list_task_agents(&task.id)
        .await
        .unwrap()
        .into_iter()
        .map(|a| {
            let mut pin = test_pin();
            pin.model = a.model;
            NewTaskAgent::new(a.step, ["code-review"], pin)
        })
        .collect();
    h.store
        .update_task(
            &task.id,
            ariadne_store::TaskUpdate {
                agents: Some(agents),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let orchestrator = h.launcher.spawn_orchestrator(&task.goal_id).await.unwrap();
    let briefing = h
        .launch_file(&orchestrator.id)
        .unwrap()
        .initial_prompt
        .unwrap();
    assert!(briefing.contains("Workflow: develop-review-merge"));
    for column in [
        "- develop [Develop]:",
        "- review [Review]:",
        "- merge [Merge]:",
    ] {
        assert!(briefing.lines().any(|line| line.starts_with(column)));
    }
    assert!(!briefing.contains("Landing:"));
    let _scheduler =
        ariadne_daemon::scheduler::start(h.store.clone(), h.launcher.clone(), false, h.timeouts);
    let develop = session_at(&h, &task, "develop").await;
    let launch = h.launch_file(&develop.id).unwrap();
    let skills = h.launcher.cfg.run_dir.join(&develop.id).join("skills");
    assert!(launch.system_prompt.contains("- code-review:"));
    assert!(!launch.system_prompt.contains("- coding:"));
    assert!(skills.join("code-review/SKILL.md").is_file());
    assert!(!skills.join("coding/SKILL.md").exists());
}

#[tokio::test]
async fn an_invalid_stored_gate_refuses_completion_without_moving_the_task() {
    use sqlx::Connection;

    let h = harness().scheduler().await;
    let task = stepped(&h).await;
    let develop = session_at(&h, &task, "develop").await;
    let mut db = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}",
        h.dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE goal_steps SET gate = 'unknown' WHERE goal_id = ? AND id = 'develop'")
        .bind(&task.goal_id)
        .execute(&mut db)
        .await
        .unwrap();
    let before = h.store.list_task_transitions(&task.id).await.unwrap().len();
    let refused = h
        .error(
            as_session(
                &format!("/v1/tasks/{}/step/complete", task.id),
                &develop.id,
                json!({"reason":"Ready."}),
            ),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(refused.error.code, "step_gate_failed");
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().step.as_deref(),
        Some("develop")
    );
    assert_eq!(
        h.store.list_task_transitions(&task.id).await.unwrap().len(),
        before
    );
}

#[tokio::test]
async fn invalid_stored_columns_return_goal_errors() {
    use sqlx::Connection;

    let h = harness().await;
    let task = stepped(&h).await;
    let mut db = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}",
        h.dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    for (skills, rank, gate, field) in [
        ("{}", "fast", "committed", "skills"),
        ("[]", "unknown", "committed", "rank"),
        ("[]", "fast", "unknown", "gate"),
    ] {
        sqlx::query("UPDATE goal_steps SET skills = ?, rank = ?, gate = ? WHERE goal_id = ? AND id = 'develop'")
            .bind(skills).bind(rank).bind(gate).bind(&task.goal_id).execute(&mut db).await.unwrap();
        let refused = h
            .error(
                crate::common::get(&format!("/v1/goals/{}", task.goal_id)),
                StatusCode::BAD_REQUEST,
            )
            .await;
        assert!(refused.error.message.contains(field));
    }
}
