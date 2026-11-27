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

    let sign_in = get(&app, "/admin", None).await;
    assert_eq!(sign_in.status(), StatusCode::OK);
    assert!(!body_text(sign_in).await.contains("googletagmanager.com"));

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
    draft_request(path, csrf, cookie, "Test", "News", "Body", &[])
}

fn draft_request(
    path: &str,
    csrf: &str,
    cookie: Option<&str>,
    title: &str,
    category: &str,
    content: &str,
    fields: &[(&str, &str)],
) -> Request<Body> {
    let boundary = "tinythought-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\n{title}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\n{category}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\n{content}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{csrf}\r\n"
    );
    for (name, value) in fields {
        body.push_str(&format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    let mut request = Request::builder().method("POST").uri(path).header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={boundary}"),
    );
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    request.body(Body::from(body)).unwrap()
}

fn signed_in_app(pool: &PgPool) -> (axum::Router, String, String) {
    let salt = SaltString::encode_b64(&[7; 16]).unwrap();
    let hash = Argon2::default()
        .hash_password(b"test password", &salt)
        .unwrap()
        .to_string();
    let auth = AdminAuth::new("owner".to_string(), hash, false).unwrap();
    let categories = Arc::new(PgCategoryRepository::new(pool.clone()));
    let posts = Arc::new(PgPostRepository::new(pool.clone()));
    let state = build_state(
        PathBuf::from("public/content"),
        auth,
        Arc::new(CategoryService::new(categories.clone())),
        Arc::new(PostService::new(posts.clone())),
        Arc::new(MigrationService::new(categories, posts)),
    );
    let (session, csrf) = state.admin_auth.create_session();
    let cookie = format!("{}={session}", state.admin_auth.cookie_name());
    (build_router(state), cookie, csrf)
}

async fn get(app: &axum::Router, path: &str, cookie: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[sqlx::test]
async fn publishing_inline_images_preserves_their_placement_and_serves_them(pool: PgPool) {
    let (app, cookie, csrf) = signed_in_app(&pool);

    let missing = get(&app, "/admin/missing", None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(!body_text(missing).await.contains("googletagmanager.com"));

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
    let preview_url = response.headers()[header::LOCATION].to_str().unwrap();
    let draft_url = preview_url.strip_suffix("/preview").unwrap();
    let dashboard = body_text(get(&app, "/admin", Some(&cookie)).await).await;
    assert!(!dashboard.contains("googletagmanager.com"));
    assert!(dashboard.contains(preview_url));
    assert!(dashboard.contains("Image test"));
    assert_eq!(
        get(&app, preview_url, None).await.status(),
        StatusCode::SEE_OTHER
    );
    let preview = get(&app, preview_url, Some(&cookie)).await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()[header::CACHE_CONTROL], "no-store");
    let preview = body_text(preview).await;
    assert!(!preview.contains("googletagmanager.com"));
    assert!(preview.contains("noindex, nofollow"));
    assert!(preview.contains("Draft preview"));
    let image_prefix = format!("{draft_url}/image/");
    let filenames: Vec<_> = preview
        .split(&image_prefix)
        .skip(1)
        .map(|image| image.split('"').next().unwrap())
        .collect();
    let [filename, second_filename] = filenames.as_slice() else {
        panic!("expected two private image URLs");
    };
    assert_ne!(filename, second_filename);
    assert!(preview.contains(&format!(
        "<p>Opening</p>\n<p><img src=\"{image_prefix}{filename}\" alt=\"First photo\" /></p>\n<p>Middle</p>"
    )));
    assert!(preview.contains(&format!(
        "<p>Middle</p>\n<p><img src=\"{image_prefix}{second_filename}\" alt=\"Second photo\" /></p>\n<p>Closing</p>"
    )));
    let private_path = format!("{image_prefix}{filename}");
    let public_path = format!("/post-images/{filename}");
    let second_public_path = format!("/post-images/{second_filename}");
    assert_eq!(
        get(&app, &private_path, None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, &public_path, None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, &second_public_path, None).await.status(),
        StatusCode::NOT_FOUND
    );
    let private_image = get(&app, &private_path, Some(&cookie)).await;
    assert_eq!(private_image.status(), StatusCode::OK);
    assert_eq!(private_image.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        get(&app, "/posts/image-test", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app, "/news", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        !body_text(get(&app, "/", None).await)
            .await
            .contains("Image test")
    );
    assert!(
        !body_text(get(&app, "/sitemap.xml", None).await)
            .await
            .contains("/posts/image-test")
    );

    let reordered = format!(
        "Opening\n\n![Second photo]({second_public_path})\n\nMiddle\n\n![First photo]({public_path})\n\nClosing"
    );
    let saved = app
        .clone()
        .oneshot(draft_request(
            &format!("{draft_url}/edit"),
            &csrf,
            Some(&cookie),
            "Image test",
            "News",
            &reordered,
            &[("revision", "1")],
        ))
        .await
        .unwrap();
    assert_eq!(saved.headers()[header::LOCATION], preview_url);

    let publish_url = format!("{draft_url}/publish");
    let response = app
        .clone()
        .oneshot(form_request(
            &publish_url,
            "csrf_token=wrong&revision=2",
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(form_request(
            &publish_url,
            &format!("csrf_token={csrf}&revision=2"),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/admin?posted=1");
    assert_eq!(
        get(&app, preview_url, Some(&cookie)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        body_text(get(&app, "/sitemap.xml", None).await)
            .await
            .contains("/posts/image-test")
    );

    let raw: String =
        sqlx::query_scalar("SELECT body_markdown FROM posts WHERE slug = 'image-test'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(raw.contains(&public_path));
    assert!(raw.contains(&second_public_path));
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
    let body = body_text(detail).await;
    assert!(body.contains("event: 'gtm.js'"));
    for path in ["/", "/news"] {
        let page = get(&app, path, None).await;
        assert_eq!(page.status(), StatusCode::OK);
        assert!(body_text(page).await.contains("event: 'gtm.js'"));
    }
    assert!(body.contains(&format!(
        "<p>Middle</p>\n<p><img src=\"{public_path}\" alt=\"First photo\" /></p>\n<p>Closing</p>"
    )));
    assert!(body.contains(&format!("src=\"{public_path}\"")));
    assert_eq!(
        get(&app, &second_public_path, None).await.status(),
        StatusCode::OK
    );

    let served = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&public_path)
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
}

#[sqlx::test]
async fn edited_posts_keep_the_live_version_until_the_reviewed_draft_is_published(pool: PgPool) {
    let (app, cookie, csrf) = signed_in_app(&pool);
    let category_id: i64 = sqlx::query_scalar(
        "INSERT INTO categories (slug, title) VALUES ('news', 'News') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let post_id: i64 = sqlx::query_scalar(
        "INSERT INTO posts (slug, title, category_id, body_markdown, body_html, created_at)
         VALUES ('original', 'Original title', $1, $2, '<p>Original body</p>', '2020-06-15T23:59:00Z')
         RETURNING id",
    )
    .bind(category_id)
    .bind("Original body\n\n```swift\nlet value = 1\n```")
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO posts (slug, title, category_id, body_markdown, body_html, created_at)
         VALUES ('another', 'Another title', $1, 'Another body', '<p>Another body</p>', '2021-01-01T00:00:00Z')",
    )
    .bind(category_id)
    .execute(&pool)
    .await
    .unwrap();

    let edit_post_url = format!("/admin/posts/{post_id}/edit");
    let editor = body_text(get(&app, &edit_post_url, Some(&cookie)).await).await;
    assert!(!editor.contains("googletagmanager.com"));
    assert!(editor.contains("Original body"));
    assert!(editor.contains("name=\"published_date\" value=\"2020-06-15\""));
    let invalid = app
        .clone()
        .oneshot(draft_request(
            &edit_post_url,
            &csrf,
            Some(&cookie),
            "Draft title",
            "Ideas",
            "Draft body",
            &[("published_date", "2022-02-30")],
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body_text(invalid)
            .await
            .contains("Enter a valid published date.")
    );
    let saved = app
        .clone()
        .oneshot(draft_request(
            &edit_post_url,
            &csrf,
            Some(&cookie),
            "Draft title",
            "Ideas",
            "**Draft body**\n\n```json\n{\"ok\": true}\n```",
            &[("published_date", "2022-02-03")],
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::SEE_OTHER);
    let preview_url = saved.headers()[header::LOCATION].to_str().unwrap();
    let draft_url = preview_url.strip_suffix("/preview").unwrap();
    let edit_draft_url = format!("{draft_url}/edit");
    let editor = get(&app, &edit_post_url, Some(&cookie)).await;
    assert_eq!(editor.headers()[header::LOCATION], edit_draft_url);
    let preview = body_text(get(&app, preview_url, Some(&cookie)).await).await;
    assert!(preview.contains("<strong>Draft body</strong>"));
    assert!(preview.contains("Feb 3, 2022"));
    assert!(preview.contains("syntax-constant"));
    let draft_editor = body_text(get(&app, &edit_draft_url, Some(&cookie)).await).await;
    assert!(draft_editor.contains("name=\"published_date\" value=\"2022-02-03\""));
    let live = body_text(get(&app, "/posts/original", None).await).await;
    assert!(live.contains("Original body"));
    assert!(live.contains("Jun 15, 2020"));
    assert!(live.contains("syntax-storage"));
    assert!(!live.contains("Draft body"));
    let home = body_text(get(&app, "/", None).await).await;
    assert!(home.find("Another title").unwrap() < home.find("Original title").unwrap());
    assert_eq!(
        get(&app, "/ideas", None).await.status(),
        StatusCode::NOT_FOUND
    );

    let saved = app
        .clone()
        .oneshot(draft_request(
            &edit_draft_url,
            &csrf,
            Some(&cookie),
            "Updated title",
            "Ideas",
            "**Updated body**",
            &[("revision", "1")],
        ))
        .await
        .unwrap();
    assert_eq!(saved.headers()[header::LOCATION], preview_url);
    let publish_url = format!("{draft_url}/publish");
    let stale = app
        .clone()
        .oneshot(form_request(
            &publish_url,
            &format!("csrf_token={csrf}&revision=1"),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert!(
        body_text(stale)
            .await
            .contains("<strong>Updated body</strong>")
    );
    assert!(
        body_text(get(&app, "/posts/original", None).await)
            .await
            .contains("Original body")
    );

    let published = app
        .clone()
        .oneshot(form_request(
            &publish_url,
            &format!("csrf_token={csrf}&revision=2"),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(published.headers()[header::LOCATION], "/admin?posted=1");
    let live = get(&app, "/posts/original", None).await;
    assert_eq!(live.status(), StatusCode::OK);
    let live = body_text(live).await;
    assert!(live.contains("<strong>Updated body</strong>"));
    assert!(live.contains("Feb 3, 2022"));
    let home = body_text(get(&app, "/", None).await).await;
    assert!(home.find("Updated title").unwrap() < home.find("Another title").unwrap());
    assert!(
        body_text(get(&app, "/ideas", None).await)
            .await
            .contains("Updated title")
    );

    let saved = app
        .clone()
        .oneshot(draft_request(
            &edit_post_url,
            &csrf,
            Some(&cookie),
            "Discarded title",
            "Ideas",
            "Discarded body",
            &[],
        ))
        .await
        .unwrap();
    let preview_url = saved.headers()[header::LOCATION].to_str().unwrap();
    let discard_url = preview_url.strip_suffix("/preview").unwrap().to_owned() + "/discard";
    let discarded = app
        .clone()
        .oneshot(form_request(
            &discard_url,
            &format!("csrf_token={csrf}"),
            Some(&cookie),
        ))
        .await
        .unwrap();
    assert_eq!(discarded.headers()[header::LOCATION], "/admin");
    assert_eq!(
        get(&app, preview_url, Some(&cookie)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        body_text(get(&app, "/posts/original", None).await)
            .await
            .contains("<strong>Updated body</strong>")
    );
}

fn image_request(csrf: &str, cookie: &str, image: &[u8]) -> Request<Body> {
    let boundary = "tinythought-image-test";
    let content = "Opening\n\n![First photo](upload-image:1)\n\nMiddle\n\n![Second photo](upload-image:2)\n\nClosing";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nImage test\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\nNews\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\n{content}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"csrf_token\"\r\n\r\n{csrf}\r\n"
    )
    .into_bytes();
    for slot in [2, 1] {
        body.extend_from_slice(format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image-{slot}\"; filename=\"photo.png\"\r\nContent-Type: image/png\r\n\r\n"
        ).as_bytes());
        body.extend_from_slice(image);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
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
