use askama::Template;

use crate::models::{Category, Post};

/// Shared by every page: nav state plus the per-page SEO fields every
/// template needs (title, description, canonical/OG URL, robots).
/// Centralizing these here means `<title>` and `og:title`/`og:description`
/// can never drift apart — base.html renders them from one source.
pub trait NavContext {
    fn active_category(&self) -> Option<&str>;
    fn site_url(&self) -> &str;
    fn page_title(&self) -> String;
    fn meta_description(&self) -> String;

    fn is_active(&self, slug: &str) -> bool {
        self.active_category() == Some(slug)
    }

    /// Path (leading slash, no origin) this page is canonically served at.
    fn canonical_path(&self) -> String {
        "/".to_string()
    }

    fn canonical_url(&self) -> String {
        format!("{}{}", self.site_url(), self.canonical_path())
    }

    fn og_type(&self) -> &'static str {
        "website"
    }

    fn robots(&self) -> &'static str {
        "index, follow"
    }
}

const SITE_DESCRIPTION: &str = "Short, no-noise posts on software development and life lessons.";

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate<'a> {
    pub categories: &'a [Category],
    pub posts: &'a [Post],
    pub site_url: &'a str,
}

impl NavContext for IndexTemplate<'_> {
    fn active_category(&self) -> Option<&str> {
        None
    }
    fn site_url(&self) -> &str {
        self.site_url
    }
    fn page_title(&self) -> String {
        "Mini Blog".to_string()
    }
    fn meta_description(&self) -> String {
        SITE_DESCRIPTION.to_string()
    }
}

#[derive(Template)]
#[template(path = "category.html")]
pub struct CategoryTemplate<'a> {
    pub categories: &'a [Category],
    pub category: &'a Category,
    pub posts: &'a [Post],
    pub site_url: &'a str,
}

impl NavContext for CategoryTemplate<'_> {
    fn active_category(&self) -> Option<&str> {
        Some(&self.category.slug)
    }
    fn site_url(&self) -> &str {
        self.site_url
    }
    fn page_title(&self) -> String {
        format!("{} · Mini Blog", self.category.name)
    }
    fn meta_description(&self) -> String {
        format!(
            "{} short posts in {} on Mini Blog.",
            self.category.post_count, self.category.name
        )
    }
    fn canonical_path(&self) -> String {
        format!("/{}", self.category.slug)
    }
}

#[derive(Template)]
#[template(path = "post.html")]
pub struct PostTemplate<'a> {
    pub categories: &'a [Category],
    pub post: &'a Post,
    pub site_url: &'a str,
}

impl NavContext for PostTemplate<'_> {
    fn active_category(&self) -> Option<&str> {
        Some(&self.post.category)
    }
    fn site_url(&self) -> &str {
        self.site_url
    }
    fn page_title(&self) -> String {
        format!("{} · Mini Blog", self.post.title)
    }
    fn meta_description(&self) -> String {
        self.post.excerpt.clone()
    }
    fn canonical_path(&self) -> String {
        format!("/posts/{}", self.post.slug)
    }
    fn og_type(&self) -> &'static str {
        "article"
    }
}

impl PostTemplate<'_> {
    /// schema.org Article JSON-LD for Google rich results. Built with
    /// `serde_json` (not the Askama `json` filter) so title/excerpt get real
    /// JSON string escaping, not HTML escaping — `<script>` content isn't
    /// HTML-entity-decoded, so `&quot;` in there would just be broken JSON.
    pub fn ld_json(&self) -> String {
        serde_json::to_string(&serde_json::json!({
            "@context": "https://schema.org",
            "@type": "Article",
            "headline": self.post.title,
            "description": self.post.excerpt,
            "datePublished": self.post.date.to_rfc3339(),
            "url": self.canonical_url(),
        }))
        .unwrap_or_default()
    }
}

#[derive(Template)]
#[template(path = "404.html")]
pub struct NotFoundTemplate<'a> {
    pub categories: &'a [Category],
    pub site_url: &'a str,
}

impl NavContext for NotFoundTemplate<'_> {
    fn active_category(&self) -> Option<&str> {
        None
    }
    fn site_url(&self) -> &str {
        self.site_url
    }
    fn page_title(&self) -> String {
        "Not found · Mini Blog".to_string()
    }
    fn meta_description(&self) -> String {
        "The page you're looking for doesn't exist or has been moved.".to_string()
    }
    fn robots(&self) -> &'static str {
        "noindex, nofollow"
    }
}
