use std::fs;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};

use crate::SharedState;
use crate::repositories::{AppError, CategoryTable, CreatePost, Frontmatter};
use crate::views::{CategoryTemplate, IndexTemplate, NotFoundTemplate, PostTemplate};
use axum::response::{Html, IntoResponse, Json, Response};

#[axum::debug_handler]
pub async fn home(State(state): State<SharedState>) -> Result<Html<String>, AppError> {
    let categories = &state.category_services.get_categories().await.unwrap();
    let posts = &state.post_services.get_posts().await.unwrap();
    let tmpl = IndexTemplate {
        categories: &categories,
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
    let category = &state
        .category_services
        .get_categories_by_slug(&slug)
        .await?
        .unwrap();
    let posts: Vec<_> = all_posts
        .iter()
        .filter(|p| p.category_slug == slug)
        .cloned()
        .collect();

    let tmpl = CategoryTemplate {
        categories: &categories,
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
                categories: &categories,
                post,
                site_url: &state.site_url,
            };
            Ok(Html(tmpl.render().expect("post template renders")).into_response())
        }
        Err(_) => {
            return Ok(not_found_response(&categories, &state.site_url));
        }
    }
}

pub async fn not_found(State(state): State<SharedState>) -> Response {
    let categories = &state.category_services.get_categories().await.unwrap();
    not_found_response(&categories, &state.site_url)
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
    let category = &payload.category;
    if let Err(e) = safe_component(category) {
        return Ok((StatusCode::BAD_REQUEST, e.to_string()).into_response());
    }

    let slug = payload.title.to_lowercase().replace(' ', "-");
    if let Err(e) = safe_component(&slug) {
        return Ok((StatusCode::BAD_REQUEST, e.to_string()).into_response());
    }

    let doc = Frontmatter {
        title: payload.clone().title,
        slug: Some(slug.to_string()),
        categories: Vec::new(),
        tags: Vec::new(),
        draft: false,
    };

    let yaml = serde_yaml::to_string(&doc).expect("failed to encode Frontmatter");
    let file_content = format!("---\n{yaml}---\n{}", payload.content);
    let mut content_dir = state.content_dir.clone();
    content_dir.push(category);
    fs::create_dir_all(&content_dir).ok();
    content_dir.push(format!("{}.md", slug));
    let status = match fs::write(content_dir, file_content) {
        Ok(_) => StatusCode::CREATED.into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    };
    Ok(status)
}

fn safe_component(s: &str) -> Result<&str, std::io::Error> {
    let is_safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');

    if is_safe {
        Ok(s)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unsafe component is {s:?}"),
        ))
    }
}

fn not_found_response(categories: &Vec<CategoryTable>, site_url: &str) -> Response {
    let tmpl = NotFoundTemplate {
        categories: categories,
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
