use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use miniblog::{build_router, build_state};
use tower::ServiceExt;

fn content_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public/content")
}

async fn get(path: &str) -> (StatusCode, String) {
    let state = build_state(content_dir());
    let app = build_router(state);
    let response = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn home_lists_categories_and_published_posts() {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("#ACID"));
    assert!(body.contains("#Less"));
    assert!(body.contains("Software Development"));
    assert!(body.contains("Life Lessons"));
    assert!(!body.contains("#WorkInProgress"), "drafts must not be listed");
}

#[tokio::test]
async fn category_page_filters_to_its_own_posts() {
    let (status, body) = get("/software-development").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("#ACID"));
    assert!(body.contains("#DRY"));
    assert!(!body.contains("#Less"), "other categories must not leak in");
}

#[tokio::test]
async fn unknown_category_returns_404() {
    let (status, body) = get("/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("404"));
}

#[tokio::test]
async fn post_detail_renders_markdown_body() {
    let (status, body) = get("/posts/acid").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("#ACID"));
    assert!(body.contains("Atomicity, Consistency, Isolation, Durability"));
}

#[tokio::test]
async fn draft_post_is_not_routable() {
    let (status, _) = get("/posts/work-in-progress").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_post_returns_404() {
    let (status, _) = get("/posts/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn static_assets_are_served() {
    let (status, body) = get("/static/style.css").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("--primary"));
}
