//! Session locations survive daemon database connections.
use ariadne_store::{NewSession, Store};

#[tokio::test]
async fn the_session_place_survives_a_store_restart() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("store.db");
    let store = Store::open(&path).await.unwrap();
    let session = store
        .create_session(NewSession {
            goal_id: None,
            task_id: None,
            seat: None,
            task_agent_id: None,
            model: "stub:model".into(),
            effort: None,
            worktree_path: None,
            pull_request_id: None,
        })
        .await
        .unwrap();
    assert!(session.place.is_none());
    store.set_session_place(&session.id, "tmux").await.unwrap();
    store.close().await;
    let store = Store::open(&path).await.unwrap();
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .place
            .as_deref(),
        Some("tmux")
    );
}
