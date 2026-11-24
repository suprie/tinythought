use std::path::PathBuf;
use std::sync::Arc;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, SaltString};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use miniblog::auth::AdminAuth;
use miniblog::repositories::{PgCategoryRepository, PgPostRepository};
use miniblog::services::{CategoryService, MigrationService, PostService};
use miniblog::{build_router, build_state};

#[tokio::test]
async fn admin_session_requires_csrf_and_is_revoked_on_logout() {
    let salt = SaltString::encode_b64(&[9; 16]).unwrap();
    let hash = Argon2::default()
        .hash_password(b"test password for owner", &salt)
        .unwrap()
        .to_string();
    let auth = AdminAuth::new("owner".to_string(), hash.clone(), false).unwrap();
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@localhost:5432/unused")
        .unwrap();
    let categories = Arc::new(PgCategoryRepository::new(pool.clone()));
    let posts = Arc::new(PgPostRepository::new(pool));
    let state = build_state(
        PathBuf::from("public/content"),
        auth,
        Arc::new(CategoryService::new(categories.clone())),
        Arc::new(PostService::new(posts.clone())),
        Arc::new(MigrationService::new(categories, posts)),
    );
    let app = build_router(state.clone());

    let bad_login = app
        .clone()
        .oneshot(form_request(
            "/admin/login",
            "username=owner&password=wrong",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(bad_login.status(), StatusCode::SEE_OTHER);
    assert!(bad_login.headers().get(header::SET_COOKIE).is_none());

    let login = app
        .clone()
        .oneshot(form_request(
            "/admin/login",
            "username=owner&password=test+password+for+owner",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let set_cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    let cookie = set_cookie.split(';').next().unwrap();
    assert!(!cookie.contains("test password for owner"));
    assert!(!cookie.contains(&hash));
    let session_id = cookie.split_once('=').unwrap().1;
    let csrf = state.admin_auth.session_csrf(session_id).unwrap();

    let rejected = app
        .clone()
        .oneshot(post_request("/admin/posts", "wrong", Some(cookie)))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

    let logout = app
        .clone()
        .oneshot(form_request(
            "/admin/logout",
            &format!("csrf_token={csrf}"),
            Some(cookie),
        ))
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::SEE_OTHER);
    assert!(state.admin_auth.session_csrf(session_id).is_none());

    let rejected_after_logout = app
        .oneshot(post_request("/admin/posts", &csrf, Some(cookie)))
        .await
        .unwrap();
    assert_eq!(rejected_after_logout.status(), StatusCode::FORBIDDEN);
}

fn form_request(path: &str, body: &str, cookie: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    request.body(Body::from(body.to_owned())).unwrap()
}

fn post_request(path: &str, csrf: &str, cookie: Option<&str>) -> Request<Body> {
    let boundary = "tinythought-test-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nTest\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\nNews\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\nBody\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{csrf}\r\n\
         --{boundary}--\r\n"
    );
    let mut request = Request::builder().method("POST").uri(path).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={boundary}"),
    );
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    request.body(Body::from(body)).unwrap()
}

#[sqlx::test]
async fn publishing_with_image_saves_and_serves_it(pool: PgPool) {
    let salt = SaltString::encode_b64(&[7; 16]).unwrap();
    let hash = Argon2::default()
        .hash_password(b"test password", &salt)
        .unwrap()
        .to_string();
    let auth = AdminAuth::new("owner".to_string(), hash, false).unwrap();
    let categories = Arc::new(PgCategoryRepository::new(pool.clone()));
    let posts = Arc::new(PgPostRepository::new(pool.clone()));
    let mut state = build_state(
        PathBuf::from("public/content"),
        auth,
        Arc::new(CategoryService::new(categories.clone())),
        Arc::new(PostService::new(posts.clone())),
        Arc::new(MigrationService::new(categories, posts)),
    );
    let upload_dir =
        std::env::temp_dir().join(format!("tinythought-test-{:032x}", rand::random::<u128>()));
    Arc::get_mut(&mut state).unwrap().upload_dir = upload_dir.clone();
    let (session, csrf) = state.admin_auth.create_session();
    let cookie = format!("{}={session}", state.admin_auth.cookie_name());
    let app = build_router(state);

    let mut image = Vec::new();
    let mut encoder = png::Encoder::new(&mut image, 2000, 1000);
    encoder
        .add_text_chunk("Comment".into(), "private metadata".into())
        .unwrap();
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&vec![128; 2000 * 1000]).unwrap();
    writer.finish().unwrap();
    let response = app
        .clone()
        .oneshot(image_request(&csrf, &cookie, &image))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/admin?posted=1");

    let raw: String =
        sqlx::query_scalar("SELECT body_markdown FROM posts WHERE slug = 'image-test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let filename = raw
        .split("/uploads/")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    assert!(filename.ends_with(".png"));
    let detail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/posts/image-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let body = http_body_util::BodyExt::collect(detail.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains(&format!("src=\"/uploads/{filename}\""))
    );

    let served = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/uploads/{filename}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(
        served.headers()[header::CACHE_CONTROL],
        "public, max-age=3600"
    );
    let bytes = http_body_util::BodyExt::collect(served.into_body())
        .await
        .unwrap()
        .to_bytes();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1600, 800));
    let pixels = decoded.to_luma8();
    assert_eq!(pixels.get_pixel(0, 0)[0], 128);
    assert!(
        pixels
            .enumerate_pixels()
            .any(|(x, y, pixel)| { x > 1350 && y > 735 && pixel[0] > 220 })
    );
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    assert!(reader.info().uncompressed_latin1_text.is_empty());

    let missing = app
        .oneshot(
            Request::builder()
                .uri("/uploads/missing.png")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(!missing.headers().contains_key(header::CACHE_CONTROL));
    std::fs::remove_dir_all(upload_dir).unwrap();
}

fn image_request(csrf: &str, cookie: &str, image: &[u8]) -> Request<Body> {
    let boundary = "tinythought-image-test";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nImage test\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\nNews\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\nBody\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{csrf}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\n"
    ).into_bytes();
    body.extend_from_slice(image);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri("/admin/posts")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(header::COOKIE, cookie)
        .body(Body::from(body))
        .unwrap()
}
