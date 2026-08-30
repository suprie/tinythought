use askama::Template;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use chrono::Utc;

use crate::SharedState;
use crate::repositories::{AppError, CategoryTable, CreatePost, Post};
use crate::views::{CategoryTemplate, IndexTemplate, NotFoundTemplate, PostTemplate};
use axum::response::{Html, IntoResponse, Json, Response};

#[axum::debug_handler]
pub async fn home(State(state): State<SharedState>) -> Result<Html<String>, AppError> {
    let categories = &state.category_services.get_categories().await.unwrap();
    let posts = &state.post_services.get_posts().await.unwrap();
    let tmpl = IndexTemplate {
        categories,
        posts,
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

#[axum::debug_handler]
pub async fn migrate(State(state): State<SharedState>) -> Result<Response, AppError> {
    let (categories, posts) = {
        let index = state
            .index
            .read()
            .expect("content index lock should not be poisoned");

        (index.categories.clone(), index.all_posts.clone())
    };

    state.migration_services.migrate(&categories, &posts).await;

    Ok((StatusCode::SERVICE_UNAVAILABLE).into_response())
}

pub async fn posts(
    State(state): State<SharedState>,
    Json(payload): Json<CreatePost>,
) -> Result<Response, AppError> {
    let category_slug = &payload.category.to_lowercase().replace(" ", "-");
    let slug = &payload.title.to_lowercase().replace(' ', "-");
    let post = Post {
        title: payload.title,
        slug: slug.clone(),
        category: payload.category,
        categories: Vec::new(),
        tags: Vec::new(),
        draft: false,
        date: Utc::now(),
        excerpt: "".to_string(),
        html: "".to_string(),
        raw: payload.content,
    };
    let category = state
        .category_services
        .find_or_create_category(&category_slug)
        .await
        .expect("category services find and create should be success");

    match state
        .post_services
        .create_new_post(&post, category.id)
        .await
    {
        Ok(()) => Ok(StatusCode::CREATED.into_response()),
        Err(_) => Ok(StatusCode::BAD_REQUEST.into_response()),
    }
}

fn not_found_response(categories: &Vec<CategoryTable>, site_url: &str) -> Response {
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
