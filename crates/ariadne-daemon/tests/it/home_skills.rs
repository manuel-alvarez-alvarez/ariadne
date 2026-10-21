//! Integration tests for the shared copy of skill documents under the home.

use crate::common;

use axum::http::StatusCode;
use tracing_subscriber::layer::SubscriberExt;

use ariadne_api::logs::LogSnapshotResponse;
use ariadne_api::skills::SkillDto;

use common::{TIMEOUT, delete, eventually, get, harness, post, post_json, put_json};

/// The home copy begins as the store's catalog, follows every document
/// mutation, and clears a folder the store no longer names.
#[tokio::test]
async fn the_home_skill_copy_tracks_the_catalog_and_its_documents() {
    let h = harness().await;
    let skills_dir = h.launcher.cfg.skills_dir.clone();
    let native_homes = [
        h.launcher.cfg.native_home.join(".claude/skills"),
        h.launcher.cfg.native_home.join(".agents/skills"),
    ];

    let skills = h.store.list_skills().await.unwrap();
    for skill in skills {
        let source = skills_dir.join(&skill.name).join("SKILL.md");
        assert_eq!(
            std::fs::read_to_string(&source).unwrap(),
            skill.document_text()
        );
        for home in &native_homes {
            assert_eq!(
                std::fs::canonicalize(home.join(&skill.name).join("SKILL.md")).unwrap(),
                std::fs::canonicalize(&source).unwrap(),
            );
        }
    }

    let coding = skills_dir.join("coding");
    let custom = "---\nname: api-design\ndescription: ours\n---\nfirst copy\n";
    let _: SkillDto = h
        .json(
            post_json(
                "/v1/skills",
                serde_json::json!({ "name": "api-design", "document": custom }),
            ),
            StatusCode::CREATED,
        )
        .await;
    eventually(TIMEOUT, "the created skill in the home", || async {
        std::fs::read_to_string(skills_dir.join("api-design/SKILL.md"))
            .is_ok_and(|text| text == custom)
    })
    .await;

    let edited = "---\nname: coding\ndescription: edited\n---\nnew copy\n";
    let _: SkillDto = h
        .json(
            put_json(
                "/v1/skills/coding",
                serde_json::json!({ "document": edited }),
            ),
            StatusCode::OK,
        )
        .await;
    eventually(TIMEOUT, "the edited document in the home", || async {
        std::fs::read_to_string(coding.join("SKILL.md")).is_ok_and(|text| text == edited)
    })
    .await;

    let _: SkillDto = h
        .json(post("/v1/skills/coding/document/reset"), StatusCode::OK)
        .await;
    eventually(TIMEOUT, "the reset document in the home", || async {
        let skill = h.store.get_skill("coding").await.unwrap();
        std::fs::read_to_string(coding.join("SKILL.md"))
            .is_ok_and(|text| text == skill.document_text())
    })
    .await;

    std::fs::create_dir_all(skills_dir.join("stale-skill")).unwrap();
    for home in &native_homes {
        std::fs::create_dir_all(home.join("ours")).unwrap();
        std::fs::write(home.join("ours/keep"), "ours").unwrap();
    }
    let (status, _) = h.send(delete("/v1/skills/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!skills_dir.join("api-design").exists());
    assert!(!skills_dir.join("stale-skill").exists());
    assert!(coding.join("SKILL.md").is_file());
    for home in &native_homes {
        assert!(!home.join("api-design").exists());
        assert!(home.join("ours/keep").is_file());
        assert!(home.join("coding").is_symlink());
    }

    let _: SkillDto = h.json(get("/v1/skills/coding"), StatusCode::OK).await;
}

/// Native folders belong to their user unless the existing name is an
/// Ariadne link back into the shared skill copy. A collision is reported but
/// must not block another native directory from receiving its link.
#[tokio::test]
async fn a_user_native_skill_collision_is_logged_and_left_untouched() {
    let h = harness().await;
    let skills_dir = h.launcher.cfg.skills_dir.clone();
    let collision = h.launcher.cfg.native_home.join(".claude/skills/coding");
    std::fs::remove_file(&collision).unwrap();
    std::fs::create_dir_all(&collision).unwrap();
    std::fs::write(collision.join("keep"), "ours").unwrap();

    {
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::registry().with(h.telemetry.log_layer()),
        );
        ariadne_daemon::agents::sync_skills(
            &h.store,
            &h.launcher.cfg.skills_dir,
            &h.launcher.cfg.native_home,
        )
        .await
        .unwrap();
    }

    assert_eq!(
        std::fs::read_to_string(collision.join("keep")).unwrap(),
        "ours"
    );
    assert!(
        h.launcher
            .cfg
            .native_home
            .join(".agents/skills/coding")
            .is_symlink()
    );

    h.telemetry.flush();
    eventually(TIMEOUT, "the native collision error", || async {
        let logs: LogSnapshotResponse = h.get("/v1/logs?level=error").await;
        logs.lines.iter().any(|line| {
            line.message
                .contains("native skill path collides with a user entry")
                && line.message.contains("name=coding")
                && line.message.contains(&format!(
                    "shared_path={}",
                    std::fs::canonicalize(&skills_dir)
                        .unwrap()
                        .join("coding")
                        .display()
                ))
                && line
                    .message
                    .contains(&format!("native_path={}", collision.display()))
                && line.message.contains("entry_kind=directory")
                && line.message.contains("outcome=left_untouched")
        })
    })
    .await;
}
