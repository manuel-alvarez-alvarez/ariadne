//! The embedded API documentation and the schema it loads (012).
use crate::common::{get, harness};
use axum::http::{StatusCode, header};
use serde_json::Value;

#[tokio::test]
async fn swagger_serves_its_page_assets_and_the_daemons_schema() {
    let h = harness().await;
    let redirect = h.response(get("/docs")).await;
    assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
    assert_eq!(redirect.headers()[header::LOCATION], "/docs/");

    let (status, body) = h.send(get("/docs/")).await;
    assert_eq!(status, StatusCode::OK);
    let html = String::from_utf8(body).unwrap();
    for asset in [
        "swagger-ui.css",
        "swagger-ui-bundle.js",
        "swagger-ui-standalone-preset.js",
        "swagger-initializer.js",
    ] {
        assert!(html.contains(asset), "the page loads {asset}");
        let (status, body) = h.send(get(&format!("/docs/{asset}"))).await;
        assert_eq!(status, StatusCode::OK, "the daemon serves {asset}");
        assert!(!body.is_empty(), "{asset} is not empty");
        if asset == "swagger-initializer.js" {
            let script = String::from_utf8(body).unwrap();
            assert!(script.contains("/api-docs/openapi.json"));
        }
    }

    let doc: Value = h.get("/api-docs/openapi.json").await;
    assert!(doc["paths"]["/v1/health"]["get"].is_object());
    assert!(doc["components"]["schemas"]["HealthResponse"].is_object());
}
