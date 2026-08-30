use crate::content::Index;
use crate::services::{CategoryService, MigrationService, PostService};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub struct AppState {
    pub index: RwLock<Index>,
    pub content_dir: PathBuf,
    /// Absolute origin (no trailing slash) used for canonical/OG URLs and the
    /// sitemap. Override via `SITE_URL` in production — search engines and
    /// link-preview crawlers (LinkedIn, Slack, ...) need real absolute URLs.
    pub site_url: String,
    pub token: String,
    pub category_services: Arc<CategoryService>,
    pub migration_services: Arc<MigrationService>,
    pub post_services: Arc<PostService>,
}
