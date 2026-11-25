use askama::Template;
use axum::extract::{Form, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use chrono::{Datelike, NaiveDate, Utc};
use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::Deserialize;
use std::collections::HashMap;

use crate::repositories::{AppError, CategoryTable, DraftInput, PostDraft};
use crate::views::{
    AdminTemplate, CategoryTemplate, IndexTemplate, NotFoundTemplate, PostEditorTemplate,
    PostPreviewTemplate, PostTemplate,
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
    posts.sort_unstable_by_key(|post| {
        std::cmp::Reverse((post.published_date(), post.created_at, post.id))
    });
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
                html: &state.post_services.render_markdown(&post.raw),
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
const MAX_IMAGES_PER_SAVE: usize = 4;
pub(crate) const MAX_EDITOR_BYTES: usize = MAX_IMAGE_BYTES * MAX_IMAGES_PER_SAVE + 1024 * 1024;
const IMAGE_UPLOAD_ERROR: &str =
    "Choose up to four JPEG, PNG, GIF, or WebP files, each up to 5 MB.";

#[derive(Default)]
struct PostForm {
    title: String,
    category: String,
    content: String,
    published_date: Option<String>,
    csrf_token: String,
    images: Vec<(Option<usize>, Vec<u8>)>,
    revision: Option<i64>,
}

async fn parse_post_form(mut multipart: Multipart) -> Result<PostForm, ()> {
    let mut form = PostForm::default();
    while let Some(field) = multipart.next_field().await.map_err(|_| ())? {
        match field.name().unwrap_or_default() {
            "title" => form.title = field.text().await.map_err(|_| ())?,
            "category" => form.category = field.text().await.map_err(|_| ())?,
            "content" => form.content = field.text().await.map_err(|_| ())?,
            "published_date" => form.published_date = Some(field.text().await.map_err(|_| ())?),
            "csrf_token" => form.csrf_token = field.text().await.map_err(|_| ())?,
            "revision" => {
                form.revision = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| ())?
                        .parse()
                        .map_err(|_| ())?,
                );
            }
            name if name == "image" || name.starts_with("image-") => {
                let slot = name
                    .strip_prefix("image-")
                    .and_then(|slot| slot.parse().ok());
                let bytes = field.bytes().await.map_err(|_| ())?;
                if slot.is_some() || !bytes.is_empty() {
                    form.images.push((slot, bytes.to_vec()));
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

    let (posts, drafts, categories) = if logged_in {
        (
            state.post_services.get_posts().await.unwrap_or_default(),
            state.post_services.drafts().await.unwrap_or_default(),
            state
                .category_services
                .get_categories()
                .await
                .unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };

    let tmpl = AdminTemplate {
        categories: &categories,
        posts: &posts,
        drafts: &drafts,
        logged_in,
        posted: flash.contains_key("posted"),
        failed: flash.contains_key("failed"),
        delete_error: flash.contains_key("delete_error"),
        csrf_token: &csrf_token,
        site_url: &state.site_url,
    };
    Html(tmpl.render().expect("admin template renders")).into_response()
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

#[derive(Clone, Copy)]
enum EditorTarget {
    New,
    Post(i64),
    Draft(i64),
}

impl EditorTarget {
    fn action(self) -> String {
        match self {
            Self::New => "/admin/posts".to_owned(),
            Self::Post(id) => format!("/admin/posts/{id}/edit"),
            Self::Draft(id) => format!("/admin/drafts/{id}/edit"),
        }
    }
}

async fn render_editor(
    state: &SharedState,
    csrf_token: &str,
    target: EditorTarget,
    form: &PostForm,
    error: Option<&str>,
) -> Result<Response, AppError> {
    let categories = state.category_services.get_categories().await?;
    let tmpl = PostEditorTemplate {
        categories: &categories,
        heading: match target {
            EditorTarget::New => "New post",
            EditorTarget::Post(_) => "Edit post",
            EditorTarget::Draft(_) => "Edit draft",
        },
        action: &target.action(),
        title: &form.title,
        category: &form.category,
        raw: &form.content,
        published_date: form.published_date.as_deref().unwrap_or_default(),
        revision: form.revision,
        error,
        csrf_token,
        site_url: &state.site_url,
    };
    let status = if error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Html(tmpl.render().expect("editor template renders")),
    )
        .into_response())
}

pub async fn admin_new_editor(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let Some(csrf) = session_csrf(&state, &headers) else {
        return Ok(Redirect::to("/admin").into_response());
    };
    render_editor(&state, &csrf, EditorTarget::New, &PostForm::default(), None).await
}

pub async fn admin_post_editor(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(csrf) = session_csrf(&state, &headers) else {
        return Ok(Redirect::to("/admin").into_response());
    };
    if let Some(draft) = state.post_services.draft_for_post(id).await? {
        return Ok(Redirect::to(&format!("/admin/drafts/{}/edit", draft.id)).into_response());
    }
    let post = match state.post_services.post_by_id(id).await {
        Ok(post) => post,
        Err(error) => return Ok(admin_lookup_error(error)),
    };
    let published_date = post.published_date().to_string();
    let form = PostForm {
        title: post.title,
        category: post.category_name,
        content: post.raw,
        published_date: Some(published_date),
        ..PostForm::default()
    };
    render_editor(&state, &csrf, EditorTarget::Post(id), &form, None).await
}

pub async fn admin_draft_editor(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(csrf) = session_csrf(&state, &headers) else {
        return Ok(Redirect::to("/admin").into_response());
    };
    let draft = match state.post_services.draft(id).await {
        Ok(draft) => draft,
        Err(error) => return Ok(admin_lookup_error(error)),
    };
    let published_date = draft.preview_date().map(|date| date.to_string());
    let form = PostForm {
        title: draft.title,
        category: draft.category,
        content: draft.raw,
        published_date,
        revision: Some(draft.revision),
        ..PostForm::default()
    };
    render_editor(&state, &csrf, EditorTarget::Draft(id), &form, None).await
}

pub async fn admin_create(
    State(state): State<SharedState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, AppError> {
    save_editor(&state, &headers, EditorTarget::New, multipart).await
}

pub async fn admin_edit_post(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    save_editor(&state, &headers, EditorTarget::Post(id), multipart).await
}

pub async fn admin_edit_draft(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    save_editor(&state, &headers, EditorTarget::Draft(id), multipart).await
}

async fn save_editor(
    state: &SharedState,
    headers: &HeaderMap,
    target: EditorTarget,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let Some(csrf) = session_csrf(state, headers) else {
        return Ok(StatusCode::FORBIDDEN.into_response());
    };
    let mut form = match parse_post_form(multipart).await {
        Ok(form) => form,
        Err(()) => {
            return Ok((StatusCode::BAD_REQUEST, "Couldn't read the draft form.").into_response());
        }
    };
    if csrf != form.csrf_token {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let draft = if let EditorTarget::Draft(id) = target {
        match state.post_services.draft(id).await {
            Ok(draft) if Some(draft.revision) == form.revision => Some(draft),
            Ok(_) => return Ok(draft_conflict()),
            Err(error) => return Ok(admin_lookup_error(error)),
        }
    } else {
        None
    };
    if form.title.trim().is_empty()
        || form.category.trim().is_empty()
        || form.content.trim().is_empty()
    {
        return render_editor(
            state,
            &csrf,
            target,
            &form,
            Some("Enter a title, category, and body."),
        )
        .await;
    }

    let published_date = match form.published_date.as_deref().map(str::trim) {
        Some("") => None,
        None => draft.as_ref().and_then(|draft| draft.published_date),
        Some(value) => match value.parse::<NaiveDate>() {
            Ok(date) if (1..=9999).contains(&date.year()) => Some(date),
            _ => {
                return render_editor(
                    state,
                    &csrf,
                    target,
                    &form,
                    Some("Enter a valid published date."),
                )
                .await;
            }
        },
    };

    let mut raw = form.content.clone();
    let uploads = std::mem::take(&mut form.images);
    let images = match prepare_draft_images(&mut raw, uploads).await {
        Ok(images) => images,
        Err(message) => return render_editor(state, &csrf, target, &form, Some(message)).await,
    };
    let post_id = match target {
        EditorTarget::Post(id) => Some(id),
        _ => None,
    };
    let version = draft.as_ref().map(|draft| (draft.id, draft.revision));
    let input = DraftInput {
        title: form.title.trim().to_owned(),
        category: form.category.trim().to_owned(),
        published_date,
        referenced_images: markdown_images(&raw)
            .filter_map(|url| url.strip_prefix("/post-images/").map(str::to_owned))
            .collect(),
        raw,
        images,
    };
    match state
        .post_services
        .save_draft(post_id, version, input)
        .await
    {
        Ok(id) => Ok(Redirect::to(&format!("/admin/drafts/{id}/preview")).into_response()),
        Err(AppError::Database(sqlx::Error::RowNotFound)) => Ok(draft_conflict()),
        Err(AppError::Database(sqlx::Error::Database(error))) if error.is_unique_violation() => {
            Ok(draft_conflict())
        }
        Err(error) => Err(error),
    }
}

fn markdown_images(body: &str) -> impl Iterator<Item = pulldown_cmark::CowStr<'_>> {
    Parser::new_ext(body, Options::ENABLE_STRIKETHROUGH).filter_map(|event| match event {
        Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url),
        _ => None,
    })
}

async fn prepare_draft_images(
    raw: &mut String,
    uploads: Vec<(Option<usize>, Vec<u8>)>,
) -> Result<Vec<(String, Vec<u8>)>, &'static str> {
    if uploads.len() > MAX_IMAGES_PER_SAVE {
        return Err(IMAGE_UPLOAD_ERROR);
    }
    let mut images = Vec::new();
    for (slot, bytes) in uploads {
        let placeholder = slot.map(|slot| format!("upload-image:{slot}"));
        if let Some(url) = &placeholder
            && !markdown_images(raw).any(|image| image.as_ref() == url)
        {
            if raw.contains(&format!("]({url})")) {
                return Err("Move the image Markdown outside the code block before saving.");
            }
            continue;
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(IMAGE_UPLOAD_ERROR);
        }
        let (bytes, extension) = post_image::prepare(bytes).await.ok_or(IMAGE_UPLOAD_ERROR)?;
        let filename = format!("{:032x}.{extension}", rand::random::<u128>());
        let image_url = format!("/post-images/{filename}");
        if let Some(url) = placeholder {
            *raw = raw.replace(&format!("]({url})"), &format!("]({image_url})"));
        } else {
            raw.push_str(&format!("\n\n![Post image]({image_url})\n"));
        }
        if markdown_images(raw).any(|url| url.as_ref() == image_url) {
            images.push((filename, bytes));
        }
    }
    if markdown_images(raw).any(|url| url.starts_with("upload-image:")) {
        return Err(
            "Select the pending image files again in their original order, or remove their Markdown.",
        );
    }
    Ok(images)
}

pub async fn admin_preview(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(csrf) = session_csrf(&state, &headers) else {
        return Ok(Redirect::to("/admin").into_response());
    };
    let draft = match state.post_services.draft(id).await {
        Ok(draft) => draft,
        Err(error) => return Ok(admin_lookup_error(error)),
    };
    render_preview(&state, &csrf, &draft, StatusCode::OK, None).await
}

async fn render_preview(
    state: &SharedState,
    csrf_token: &str,
    draft: &PostDraft,
    status: StatusCode,
    error: Option<&str>,
) -> Result<Response, AppError> {
    let categories = state.category_services.get_categories().await?;
    let tmpl = PostPreviewTemplate {
        categories: &categories,
        draft,
        html: &state.post_services.render_draft(draft),
        date_label: draft
            .preview_date()
            .unwrap_or_else(|| Utc::now().date_naive())
            .format("%b %-d, %Y")
            .to_string(),
        csrf_token,
        error,
        site_url: &state.site_url,
    };
    Ok((
        status,
        Html(tmpl.render().expect("preview template renders")),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct PublishForm {
    csrf_token: String,
    revision: i64,
}

pub async fn admin_publish(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<PublishForm>,
) -> Result<Response, AppError> {
    if !valid_csrf(&state, &headers, &form.csrf_token) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let draft = match state.post_services.draft(id).await {
        Ok(draft) => draft,
        Err(error) => return Ok(admin_lookup_error(error)),
    };
    if draft.revision != form.revision {
        return render_preview(
            &state,
            &form.csrf_token,
            &draft,
            StatusCode::CONFLICT,
            Some("The draft changed. Review this version before publishing."),
        )
        .await;
    }
    match state.post_services.publish_draft(&draft).await {
        Ok(()) => Ok(Redirect::to("/admin?posted=1").into_response()),
        Err(AppError::Database(sqlx::Error::RowNotFound)) => Ok(draft_conflict()),
        Err(AppError::Database(sqlx::Error::Database(error))) if error.is_unique_violation() => {
            render_preview(
                &state,
                &form.csrf_token,
                &draft,
                StatusCode::CONFLICT,
                Some("A post already uses this URL. Edit the draft title and try again."),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

pub async fn admin_discard(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    if !valid_csrf(&state, &headers, &form.csrf_token) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    state.post_services.discard_draft(id).await?;
    Ok(Redirect::to("/admin").into_response())
}

pub async fn admin_draft_image(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Path((id, filename)): Path<(i64, String)>,
) -> Result<Response, AppError> {
    if session_csrf(&state, &headers).is_none() {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    post_image_response(&state, Some(id), &filename).await
}

pub async fn published_post_image(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<Response, AppError> {
    post_image_response(&state, None, &filename).await
}

async fn post_image_response(
    state: &SharedState,
    draft_id: Option<i64>,
    filename: &str,
) -> Result<Response, AppError> {
    let content_type = match filename.rsplit_once('.').map(|(_, ext)| ext) {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return Ok(StatusCode::NOT_FOUND.into_response()),
    };
    let Some(bytes) = state.post_services.image(draft_id, filename).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let cache = if draft_id.is_some() {
        "no-store"
    } else {
        "public, max-age=3600"
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache),
        ],
        bytes,
    )
        .into_response())
}

fn admin_lookup_error(error: AppError) -> Response {
    match error {
        AppError::Database(sqlx::Error::RowNotFound) => (
            StatusCode::NOT_FOUND,
            "This post or draft no longer exists.",
        )
            .into_response(),
        error => error.into_response(),
    }
}

fn draft_conflict() -> Response {
    (
        StatusCode::CONFLICT,
        "This draft changed. Return to the admin dashboard and reopen it.",
    )
        .into_response()
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
pub async fn sitemap_xml(State(state): State<SharedState>) -> Result<Response, AppError> {
    let categories = state.category_services.get_categories().await?;
    let posts = state.post_services.get_posts().await?;
    let site_url = &state.site_url;

    let mut urls = format!("  <url><loc>{site_url}/</loc></url>\n");
    for category in &categories {
        urls.push_str(&format!(
            "  <url><loc>{site_url}/{}</loc></url>\n",
            xml_escape(&category.slug)
        ));
    }
    for post in &posts {
        urls.push_str(&format!(
            "  <url><loc>{site_url}/posts/{}</loc><lastmod>{}</lastmod></url>\n",
            xml_escape(&post.slug),
            post.created_at.format("%Y-%m-%d")
        ));
    }

    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{urls}</urlset>\n"
    );
    Ok(([(header::CONTENT_TYPE, "application/xml")], body).into_response())
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
