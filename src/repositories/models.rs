use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
/// Raw fields parsed straight out of a post's YAML frontmatter block.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Frontmatter {
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error")]
    Database(#[source] sqlx::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Database(error) => {
                eprintln!("database error: {error}");

                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "internal server error"
                    })),
                )
                    .into_response()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Post {
    pub title: String,
    pub slug: String,
    /// Canonical category, derived from the folder the post lives in.
    pub category: String,
    /// Categories declared in frontmatter (defaults to `[category]`).
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub draft: bool,
    /// Last-modified time of the source file — doubles as the "published" date.
    pub date: chrono::DateTime<chrono::Utc>,
    pub excerpt: String,
    pub html: String,
    pub raw: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PostTable {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub category_id: i64,
    pub category_name: String,
    pub category_slug: String,
    pub html: String,
    pub raw: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub published_date: Option<chrono::NaiveDate>,
}

#[derive(sqlx::FromRow)]
pub struct PostDraft {
    pub id: i64,
    pub post_id: Option<i64>,
    pub title: String,
    pub category: String,
    pub raw: String,
    pub revision: i64,
    pub image_filenames: Vec<String>,
    pub published_date: Option<chrono::NaiveDate>,
    pub live_published_date: Option<chrono::NaiveDate>,
}

impl PostDraft {
    pub fn preview_date(&self) -> Option<chrono::NaiveDate> {
        self.published_date.or(self.live_published_date)
    }
}

pub struct DraftInput {
    pub title: String,
    pub category: String,
    pub raw: String,
    pub published_date: Option<chrono::NaiveDate>,
    pub images: Vec<(String, Vec<u8>)>,
    pub referenced_images: Vec<String>,
}

impl PostTable {
    pub fn published_date(&self) -> chrono::NaiveDate {
        self.published_date
            .unwrap_or_else(|| self.created_at.date_naive())
    }

    pub fn date_label(&self) -> String {
        self.published_date().format("%b %-d, %Y").to_string()
    }

    pub fn excerpt(&self) -> String {
        crate::content::plain_excerpt(&self.raw, 180)
    }
}

impl Post {
    pub fn date_label(&self) -> String {
        self.date.format("%b %-d, %Y").to_string()
    }
}

/// A category, auto-populated from a subdirectory of `public/content`.
#[derive(Debug, Clone)]
pub struct Category {
    pub slug: String,
    pub name: String,
    pub post_count: usize,
}

#[derive(Debug, Clone)]
pub struct CategoryTable {
    pub id: i64,
    pub slug: String,
    pub title: String,
}

impl Category {
    pub fn from_slug(slug: &str, post_count: usize) -> Self {
        let name = slug
            .split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        Category {
            slug: slug.to_string(),
            name,
            post_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_name_is_derived_from_slug() {
        assert_eq!(Category::from_slug("life-lessons", 2).name, "Life Lessons");
        assert_eq!(
            Category::from_slug("software-development", 5).name,
            "Software Development"
        );
        assert_eq!(Category::from_slug("misc", 0).name, "Misc");
    }
}
