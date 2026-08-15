use askama::Template;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

use crate::content::Index;
use crate::views::{CategoryTemplate, IndexTemplate, NotFoundTemplate, PostTemplate};
use crate::SharedState;

pub async fn home(State(state): State<SharedState>) -> Html<String> {
    let index = state.index.read().unwrap();
    let tmpl = IndexTemplate {
        categories: &index.categories,
        posts: &index.all_posts,
        site_url: &state.site_url,
    };
    Html(tmpl.render().expect("index template renders"))
}

pub async fn category_page(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Response {
    let index = state.index.read().unwrap();
    let Some(category) = index.category(&slug) else {
        return not_found_response(&index, &state.site_url);
    };
    let empty = Vec::new();
    let posts = index.posts_by_category.get(&slug).unwrap_or(&empty);
    let tmpl = CategoryTemplate {
        categories: &index.categories,
        category,
        posts,
        site_url: &state.site_url,
    };
    Html(tmpl.render().expect("category template renders")).into_response()
}

pub async fn post_detail(State(state): State<SharedState>, Path(slug): Path<String>) -> Response {
    let index = state.index.read().unwrap();
    let Some(post) = index.posts_by_slug.get(&slug) else {
        return not_found_response(&index, &state.site_url);
    };
    let tmpl = PostTemplate {
        categories: &index.categories,
        post,
        site_url: &state.site_url,
    };
    Html(tmpl.render().expect("post template renders")).into_response()
}

pub async fn not_found(State(state): State<SharedState>) -> Response {
    let index = state.index.read().unwrap();
    not_found_response(&index, &state.site_url)
}

fn not_found_response(index: &Index, site_url: &str) -> Response {
    let tmpl = NotFoundTemplate {
        categories: &index.categories,
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
    let body = format!("User-agent: *\nAllow: /\nSitemap: {}/sitemap.xml\n", state.site_url);
    ([(header::CONTENT_TYPE, "text/plain")], body).into_response()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
