use askama::Template;
use axum::extract::{Form, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;

use crate::repositories::{AppError, CategoryTable, Post};
use crate::views::{
    AdminTemplate, CategoryTemplate, IndexTemplate, NotFoundTemplate, PostTemplate,
};
use crate::{SharedState, post_image};

/// Liveness probe for Docker/Caddy. Deliberately does not touch the database.
pub async fn health() -> &'static str {
    "ok"
}

#[axum::debug_handler]
pub async fn home(State(state): State<SharedState>) -> Result<Html<String>, AppError> {
    let categories = &state.category_services.get_categories().await.unwrap();
    let mut posts = state.post_services.get_posts().await.unwrap();
    posts.sort_unstable_by_key(|post| std::cmp::Reverse((post.created_at, post.id)));
    let tmpl = IndexTemplate {
        categories,
        posts: &posts,
        site_url: &state.site_url,
    };
    Ok(Html(tmpl.render().expect("index template renders")))
}

pub async fn category_page(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Response, AppError> {
    let categories = &state.category_services.get_categories().await.unwrap();
    let all_posts = &state.post_services.get_posts().await.unwrap();
    let Some(category) = &state
        .category_services
        .get_categories_by_slug(&slug)
        .await?
    else {
        return Ok(not_found_response(categories, &state.site_url));
    };
    let posts: Vec<_> = all_posts
        .iter()
        .filter(|p| p.category_slug == slug)
        .cloned()
        .collect();

    let tmpl = CategoryTemplate {
        categories,
        category,
        posts: &posts,
        site_url: &state.site_url,
    };
    Ok(Html(tmpl.render().expect("category template renders")).into_response())
}

pub async fn post_detail(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Response, AppError> {
    let categories = &state.category_services.get_categories().await.unwrap();
    match &state.post_services.post_by_slug(&slug).await {
        Ok(post) => {
            let tmpl = PostTemplate {
                categories,
                post,
                site_url: &state.site_url,
            };
            Ok(Html(tmpl.render().expect("post template renders")).into_response())
        }
        Err(_) => Ok(not_found_response(categories, &state.site_url)),
    }
}

pub async fn not_found(State(state): State<SharedState>) -> Response {
    let categories = &state.category_services.get_categories().await.unwrap();
    not_found_response(categories, &state.site_url)
}

fn session_id(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == cookie_name).then(|| value.to_owned())
            })
        })
}

fn session_csrf(state: &SharedState, headers: &HeaderMap) -> Option<String> {
    let id = session_id(headers, state.admin_auth.cookie_name())?;
    state.admin_auth.session_csrf(&id)
}

fn valid_csrf(state: &SharedState, headers: &HeaderMap, submitted: &str) -> bool {
    session_csrf(state, headers).is_some_and(|expected| expected == submitted)
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct CsrfForm {
    pub csrf_token: String,
}

const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Default)]
struct PostForm {
    title: String,
    category: String,
    content: String,
    csrf_token: String,
    image: Option<Vec<u8>>,
}

async fn parse_post_form(mut multipart: Multipart) -> Result<PostForm, ()> {
    let mut form = PostForm::default();
    while let Some(field) = multipart.next_field().await.map_err(|_| ())? {
        match field.name().unwrap_or_default() {
            "title" => form.title = field.text().await.map_err(|_| ())?,
            "category" => form.category = field.text().await.map_err(|_| ())?,
            "content" => form.content = field.text().await.map_err(|_| ())?,
            "csrf_token" => form.csrf_token = field.text().await.map_err(|_| ())?,
            "image" => {
                if form.image.is_some() {
                    return Err(());
                }
                let bytes = field.bytes().await.map_err(|_| ())?;
                if !bytes.is_empty() {
                    if bytes.len() > MAX_IMAGE_BYTES {
                        return Err(());
                    }
                    form.image = Some(bytes.to_vec());
                }
            }
            _ => {}
        }
    }
    Ok(form)
}

/// GET /admin — login form when signed out, dashboard when signed in.
/// `?posted=1` and `?failed=1` (set by redirects below) drive the flash messages.
pub async fn admin_page(
    State(state): State<SharedState>,
    Query(flash): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let csrf_token = session_csrf(&state, &headers).unwrap_or_default();
    let logged_in = !csrf_token.is_empty();

    let (posts, categories): (Vec<_>, Vec<_>) = if logged_in {
        (
            state.post_services.get_posts().await.unwrap_or_default(),
            state
                .category_services
                .get_categories()
                .await
                .unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    let tmpl = AdminTemplate {
        categories: &categories,
        posts: &posts,
        logged_in,
        posted: flash.contains_key("posted"),
        failed: flash.contains_key("failed"),
        upload_error: flash.contains_key("upload_error"),
        delete_error: flash.contains_key("delete_error"),
        csrf_token: &csrf_token,
        site_url: &state.site_url,
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        Html(tmpl.render().expect("admin template renders")),
    )
        .into_response()
}

/// POST /admin/login — verify the password and issue a separate session ID.
pub async fn admin_login(
    State(state): State<SharedState>,
    Form(form): Form<LoginForm>,
) -> Response {
    if state
        .admin_auth
        .verify_credentials(&form.username, &form.password)
        .await
    {
        let (id, _) = state.admin_auth.create_session();
        let cookie = state.admin_auth.cookie(&id);
        ([(header::SET_COOKIE, cookie)], Redirect::to("/admin")).into_response()
    } else {
        Redirect::to("/admin?failed=1").into_response()
    }
}

/// POST /admin/logout — revoke the session and clear its cookie.
pub async fn admin_logout(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !valid_csrf(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(id) = session_id(&headers, state.admin_auth.cookie_name()) {
        state.admin_auth.revoke_session(&id);
    }
    let cookie = state.admin_auth.expired_cookie();
    ([(header::SET_COOKIE, cookie)], Redirect::to("/admin")).into_response()
}

/// POST /admin/posts — publish a post from the dashboard form.
pub async fn admin_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    let form = match parse_post_form(multipart).await {
        Ok(form) => form,
        Err(()) => return Redirect::to("/admin?upload_error=1").into_response(),
    };
    if !valid_csrf(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if form.title.trim().is_empty()
        || form.category.trim().is_empty()
        || form.content.trim().is_empty()
    {
        return Redirect::to("/admin?failed=1").into_response();
    }

    let mut content = form.content;
    let uploaded_path = if let Some(bytes) = form.image {
        let Some((bytes, extension)) = post_image::prepare(bytes).await else {
            return Redirect::to("/admin?upload_error=1").into_response();
        };
        if tokio::fs::create_dir_all(&state.upload_dir).await.is_err() {
            return Redirect::to("/admin?failed=1").into_response();
        }
        let filename = format!("{:032x}.{extension}", rand::random::<u128>());
        let path = state.upload_dir.join(&filename);
        let mut file = match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(_) => return Redirect::to("/admin?failed=1").into_response(),
        };
        use tokio::io::AsyncWriteExt;
        if file.write_all(&bytes).await.is_err() {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            return Redirect::to("/admin?failed=1").into_response();
        }
        content.push_str(&format!("\n\n![Post image](/uploads/{filename})\n"));
        Some(path)
    } else {
        None
    };

    match create_post(&state, &form.title, &form.category, &content).await {
        Ok(()) => Redirect::to("/admin?posted=1").into_response(),
        Err(_) => {
            if let Some(path) = uploaded_path {
                let _ = tokio::fs::remove_file(path).await;
            }
            Redirect::to("/admin?failed=1").into_response()
        }
    }
}

/// POST /admin/posts/{id}/delete — remove a post from the database.
pub async fn admin_delete(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !valid_csrf(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }

    if state.post_services.delete_post(id).await.is_err() {
        return Redirect::to("/admin?delete_error=1").into_response();
    }
    Redirect::to("/admin").into_response()
}

// ---- post creation from the admin form ----

/// Lowercases, keeps alphanumerics, collapses the rest into single dashes.
/// The old `replace(' ', "-")` kept punctuation in slugs; this cleans it.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if (ch == ' ' || ch == '-' || ch == '_') && !slug.is_empty() && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

async fn create_post(
    state: &SharedState,
    title: &str,
    category: &str,
    content: &str,
) -> Result<(), AppError> {
    let post = Post {
        title: title.to_string(),
        slug: slugify(title),
        category: category.to_string(),
        categories: Vec::new(),
        tags: Vec::new(),
        draft: false,
        date: Utc::now(),
        excerpt: String::new(),
        html: String::new(),
        raw: content.to_string(),
    };
    let category = state
        .category_services
        .find_or_create_category(category)
        .await?;
    state
        .post_services
        .create_new_post(&post, category.id)
        .await
}

fn not_found_response(categories: &[CategoryTable], site_url: &str) -> Response {
    let tmpl = NotFoundTemplate {
        categories,
        site_url,
    };
    (
        StatusCode::NOT_FOUND,
        Html(tmpl.render().expect("404 template renders")),
    )
        .into_response()
}

/// XML sitemap covering home, every category, and every published post —
/// lets Google discover and crawl the whole site without waiting on links.
pub async fn sitemap_xml(State(state): State<SharedState>) -> Response {
    let index = state.index.read().unwrap();
    let site_url = &state.site_url;

    let mut urls = format!("  <url><loc>{site_url}/</loc></url>\n");
    for category in &index.categories {
        urls.push_str(&format!(
            "  <url><loc>{site_url}/{}</loc></url>\n",
            xml_escape(&category.slug)
        ));
    }
    for post in &index.all_posts {
        urls.push_str(&format!(
            "  <url><loc>{site_url}/posts/{}</loc><lastmod>{}</lastmod></url>\n",
            xml_escape(&post.slug),
            post.date.format("%Y-%m-%d")
        ));
    }

    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{urls}</urlset>\n"
    );
    ([(header::CONTENT_TYPE, "application/xml")], body).into_response()
}

pub async fn robots_txt(State(state): State<SharedState>) -> Response {
    let body = format!(
        "User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n",
        state.site_url
    );
    ([(header::CONTENT_TYPE, "text/plain")], body).into_response()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
