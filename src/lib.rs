pub mod auth;
pub mod content;
mod post_image;
pub mod repositories;
pub mod routes;
pub mod services;
pub mod views;

use std::path::PathBuf;
use std::println;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::routing::{get, post};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, header},
    middleware,
    response::Response,
};
use tower_http::services::ServeDir;

use crate::services::{CategoryService, MigrationService, PostService};

pub const REINDEX_INTERVAL: Duration = Duration::from_secs(5 * 60 * 60);

pub struct AppState {
    pub index: RwLock<content::Index>,
    pub content_dir: PathBuf,
    pub upload_dir: PathBuf,
    /// Absolute origin (no trailing slash) used for canonical/OG URLs and the
    /// sitemap. Override via `SITE_URL` in production — search engines and
    /// link-preview crawlers (LinkedIn, Slack, ...) need real absolute URLs.
    pub site_url: String,
    pub admin_auth: auth::AdminAuth,
    pub category_services: Arc<CategoryService>,
    pub migration_services: Arc<MigrationService>,
    pub post_services: Arc<PostService>,
}

pub type SharedState = Arc<AppState>;

pub fn build_state(
    content_dir: PathBuf,
    admin_auth: auth::AdminAuth,
    category_services: Arc<CategoryService>,
    post_services: Arc<PostService>,
    migration_services: Arc<MigrationService>,
) -> SharedState {
    let index = content::build_index(&content_dir);
    let site_url = std::env::var("SITE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string())
        .trim_end_matches('/')
        .to_string();
    let upload_dir = std::env::var_os("UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("uploads"));
    Arc::new(AppState {
        index: RwLock::new(index),
        content_dir,
        upload_dir,
        site_url,
        admin_auth,
        category_services,
        migration_services,
        post_services,
    })
}

pub fn build_router(state: SharedState) -> Router {
    let uploads = Router::new()
        .nest_service("/uploads", ServeDir::new(state.upload_dir.clone()))
        .route_layer(middleware::map_response(|mut response: Response| async {
            if response.status().is_success() {
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=3600"),
                );
            }
            response
        }));
    Router::new()
        .route("/", get(routes::home))
        .route("/health", get(routes::health))
        .route("/posts/{slug}", get(routes::post_detail))
        .route("/{category}", get(routes::category_page))
        .route("/sitemap.xml", get(routes::sitemap_xml))
        .route("/robots.txt", get(routes::robots_txt))
        .route("/admin", get(routes::admin_page))
        .route("/admin/login", post(routes::admin_login))
        .route("/admin/logout", post(routes::admin_logout))
        .route("/admin/posts", post(routes::admin_create))
        .route("/admin/posts/{id}/delete", post(routes::admin_delete))
        .fallback(routes::not_found)
        .nest_service("/static", ServeDir::new("public/static"))
        .merge(uploads)
        .layer(DefaultBodyLimit::max(6 * 1024 * 1024))
        .with_state(state)
}

/// Reindexes `state.content_dir` every [`REINDEX_INTERVAL`], swapping the
/// index in atomically once the new one is fully built. Runs alongside
/// [`spawn_content_watcher`] as a fallback for changes it might miss (e.g. an
/// editor that writes outside the watch, or a watcher that failed to start).
pub fn spawn_reindex_loop(state: SharedState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(REINDEX_INTERVAL).await;
            let fresh = content::build_index(&state.content_dir);
            let count = fresh.all_posts.len();
            *state.index.write().unwrap() = fresh;
            println!("reindexed: {count} posts");
        }
    });
}

/// Watches `state.content_dir` for filesystem changes and reindexes
/// immediately, so a new or edited post shows up right away instead of
/// waiting on [`REINDEX_INTERVAL`]. Runs on its own OS thread (`notify`'s
/// callback isn't async) rather than a tokio task.
pub fn spawn_content_watcher(state: SharedState) {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;

    let content_dir = state.content_dir.clone();
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("content watcher failed to start: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&content_dir, RecursiveMode::Recursive) {
            eprintln!(
                "content watcher failed to watch {}: {e}",
                content_dir.display()
            );
            return;
        }

        while rx.recv().is_ok() {
            // A single save can fire several fs events (write + rename +
            // metadata...). Drain the channel for a short quiet period so
            // one edit triggers one reindex, not a burst of them.
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}

            let fresh = content::build_index(&content_dir);
            let count = fresh.all_posts.len();
            *state.index.write().unwrap() = fresh;
            println!("reindexed (content changed): {count} posts");
        }
    });
}
