// use std::path::PathBuf;

// use axum::body::Body;
// use axum::http::{Request, StatusCode};
// use http_body_util::BodyExt;
// use miniblog::{build_protected_router, build_router, build_state};
// use tower::ServiceExt;

// fn content_dir() -> PathBuf {
//     PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("public/content")
// }

// async fn get(path: &str) -> (StatusCode, String) {
//     let state = build_state(content_dir(), "test-token".to_string());
//     let app = build_router(state);
//     let response = app
//         .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
//         .await
//         .unwrap();
//     let status = response.status();
//     let bytes = response.into_body().collect().await.unwrap().to_bytes();
//     (status, String::from_utf8(bytes.to_vec()).unwrap())
// }

// #[tokio::test]
// async fn home_lists_categories_and_published_posts() {
//     let (status, body) = get("/").await;
//     assert_eq!(status, StatusCode::OK);
//     assert!(body.contains("#ACID"));
//     assert!(body.contains("#Less"));
//     assert!(body.contains("Software Development"));
//     assert!(body.contains("Life Lessons"));
//     assert!(
//         !body.contains("#WorkInProgress"),
//         "drafts must not be listed"
//     );
// }

// #[tokio::test]
// async fn category_page_filters_to_its_own_posts() {
//     let (status, body) = get("/software-development").await;
//     assert_eq!(status, StatusCode::OK);
//     assert!(body.contains("#ACID"));
//     assert!(body.contains("#DRY"));
//     assert!(!body.contains("#Less"), "other categories must not leak in");
// }

// #[tokio::test]
// async fn unknown_category_returns_404() {
//     let (status, body) = get("/does-not-exist").await;
//     assert_eq!(status, StatusCode::NOT_FOUND);
//     assert!(body.contains("404"));
// }

// #[tokio::test]
// async fn post_detail_renders_markdown_body() {
//     let (status, body) = get("/posts/acid").await;
//     assert_eq!(status, StatusCode::OK);
//     assert!(body.contains("#ACID"));
//     assert!(body.contains("Atomicity, Consistency, Isolation, Durability"));
// }

// #[tokio::test]
// async fn draft_post_is_not_routable() {
//     let (status, _) = get("/posts/work-in-progress").await;
//     assert_eq!(status, StatusCode::NOT_FOUND);
// }

// #[tokio::test]
// async fn unknown_post_returns_404() {
//     let (status, _) = get("/posts/does-not-exist").await;
//     assert_eq!(status, StatusCode::NOT_FOUND);
// }

// #[tokio::test]
// async fn static_assets_are_served() {
//     let (status, body) = get("/static/style.css").await;
//     assert_eq!(status, StatusCode::OK);
//     assert!(body.contains("--primary"));
// }

// // ---- POST /posts ----

// /// Self-cleaning temp dir so POST tests never write into the repo's real
// /// `public/content`.
// struct TempDir(std::path::PathBuf);

// impl TempDir {
//     fn new(name: &str) -> Self {
//         let dir = std::env::temp_dir().join(format!("miniblog-it-{name}-{}", std::process::id()));
//         let _ = std::fs::remove_dir_all(&dir);
//         std::fs::create_dir_all(&dir).unwrap();
//         TempDir(dir)
//     }

//     fn path(&self) -> &std::path::Path {
//         &self.0
//     }
// }

// impl Drop for TempDir {
//     fn drop(&mut self) {
//         let _ = std::fs::remove_dir_all(&self.0);
//     }
// }

// fn app_with_content_dir(dir: &std::path::Path) -> axum::Router {
//     let state = build_state(dir.to_path_buf(), "test-token".to_string());
//     build_router(state.clone()).merge(build_protected_router(state))
// }

// async fn post(app: axum::Router, bearer: Option<&str>, body: serde_json::Value) -> (StatusCode, String) {
//     let mut builder = Request::builder().method("POST").uri("/posts");
//     if let Some(token) = bearer {
//         builder = builder.header("authorization", format!("Bearer {token}"));
//     }
//     let request = builder
//         .header("content-type", "application/json")
//         .body(Body::from(body.to_string()))
//         .unwrap();
//     let response = app.oneshot(request).await.unwrap();
//     let status = response.status();
//     let bytes = response.into_body().collect().await.unwrap().to_bytes();
//     (status, String::from_utf8(bytes.to_vec()).unwrap())
// }

// #[tokio::test]
// async fn posts_creates_markdown_file_under_content_dir() {
//     let tmp = TempDir::new("posts-happy");
//     let app = app_with_content_dir(tmp.path());

//     let (status, _) = post(
//         app,
//         Some("test-token"),
//         serde_json::json!({
//             "title": "Test Post",
//             "category": "life-lessons",
//             "content": "Body text"
//         }),
//     )
//     .await;

//     assert_eq!(status, StatusCode::CREATED);
//     let written =
//         std::fs::read_to_string(tmp.path().join("life-lessons").join("test-post.md")).unwrap();
//     assert!(written.starts_with("---\n"), "frontmatter must lead the file");
//     assert!(written.contains("title: Test Post"));
//     assert!(written.contains("slug: test-post"));
//     assert!(written.ends_with("Body text"));
// }

// #[tokio::test]
// async fn posts_without_token_is_unauthorized_and_writes_nothing() {
//     let tmp = TempDir::new("posts-401");
//     let app = app_with_content_dir(tmp.path());

//     let (status, _) = post(
//         app,
//         None,
//         serde_json::json!({ "title": "Nope", "category": "misc", "content": "x" }),
//     )
//     .await;

//     assert_eq!(status, StatusCode::UNAUTHORIZED);
//     assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
// }

// #[tokio::test]
// async fn posts_rejects_traversal_category_before_touching_disk() {
//     let tmp = TempDir::new("posts-400-traversal");
//     let app = app_with_content_dir(tmp.path());

//     let (status, body) = post(
//         app,
//         Some("test-token"),
//         serde_json::json!({
//             "title": "Evil",
//             "category": "../../etc",
//             "content": "should never be written"
//         }),
//     )
//     .await;

//     assert_eq!(status, StatusCode::BAD_REQUEST);
//     assert!(body.contains("unsafe"));
//     assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
// }

// #[tokio::test]
// async fn posts_rejects_titles_whose_slug_would_be_unsafe() {
//     let tmp = TempDir::new("posts-400-slug");
//     let app = app_with_content_dir(tmp.path());

//     let (status, _) = post(
//         app,
//         Some("test-token"),
//         serde_json::json!({
//             "title": "Don't Repeat Yourself",
//             "category": "misc",
//             "content": "x"
//         }),
//     )
//     .await;

//     // Documents current behavior: punctuation isn't sanitized away, it's
//     // rejected. If this changes to sanitize-then-validate, update this test.
//     assert_eq!(status, StatusCode::BAD_REQUEST);
// }
