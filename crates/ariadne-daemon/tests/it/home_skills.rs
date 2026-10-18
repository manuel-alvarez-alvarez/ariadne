//! Integration tests for the shared copy of skill documents under the home.

use crate::common;

use axum::http::StatusCode;

use ariadne_api::skills::SkillDto;

use common::{TIMEOUT, delete, eventually, get, harness, post, post_json, put_json};

/// The home copy begins as the store's catalog, follows every document
/// mutation, and clears a folder the store no longer names.
#[tokio::test]
async fn the_home_skill_copy_tracks_the_catalog_and_its_documents() {
    let h = harness().await;
    let skills_dir = h.launcher.cfg.skills_dir.clone();

    eventually(TIMEOUT, "the seeded skill catalog in the home", || async {
        let skills = h.store.list_skills().await.unwrap();
        skills.iter().all(|skill| {
            std::fs::read_to_string(skills_dir.join(&skill.name).join("SKILL.md"))
                .is_ok_and(|text| text == skill.document_text())
        })
    })
    .await;

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
    let (status, _) = h.send(delete("/v1/skills/api-design")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "removed skill folders", || async {
        !skills_dir.join("api-design").exists()
            && !skills_dir.join("stale-skill").exists()
            && coding.join("SKILL.md").is_file()
    })
    .await;

    let _: SkillDto = h.json(get("/v1/skills/coding"), StatusCode::OK).await;
}
