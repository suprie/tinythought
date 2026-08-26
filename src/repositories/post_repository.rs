use crate::repositories::{
    Post,
    models::{AppError, PostTable},
};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::pool::Pool;
use sqlx::{Sqlite, SqlitePool};

#[async_trait]
pub trait PostRepository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<PostTable>, AppError>;
    async fn find_by_slug(&self, slug: &str) -> Result<PostTable, AppError>;

    async fn create(&self, post: Post, category_id: i64) -> Result<(), AppError>;
}

pub struct SQLitePostRepository {
    pool: Pool<Sqlite>,
}

impl SQLitePostRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PostRepository for SQLitePostRepository {
    async fn find_all(&self) -> Result<Vec<PostTable>, AppError> {
        sqlx::query_as!(
            PostTable,
            r#"
            Select
                p.id,
                p.title,
                p.slug,
                p.excerpt,
                c.id as category_id,
                c.title as category_name,
                c.slug as category_slug,
                p.body_markdown as raw,
                p.body_html as html,
                p.created_at as created_at
            FROM posts AS p
            INNER JOIN categories as C
            ON c.id = p.category_id"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)
    }

    async fn create(&self, post: Post, category_id: i64) -> Result<(), AppError> {
        sqlx::query!(
            r#"INSERT INTO posts (title, slug, excerpt, category_id, body_html, body_markdown, updated_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            &post.title,
            &post.slug,
            &post.excerpt,
            &category_id,
            &post.html,
            &post.raw,
            Utc::now().to_rfc3339(),
            Utc::now().to_rfc3339()
        )
        .execute(&self.pool)
        .await
        .map_err(AppError::Database)
        .map(|_| ())
    }

    async fn find_by_slug(&self, slug: &str) -> Result<PostTable, AppError> {
        sqlx::query_as!(
            PostTable,
            r#"
            Select
                p.id,
                p.title,
                p.slug,
                p.excerpt,
                c.id as category_id,
                c.title as category_name,
                c.slug as category_slug,
                p.body_markdown as raw,
                p.body_html as html,
                p.created_at as created_at
            FROM posts AS p
            INNER JOIN categories as C
            ON c.id = p.category_id
            WHERE p.slug = $1
            "#,
            slug
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[sqlx::test]
    async fn test_find_all(pool: Pool<Sqlite>) {
        let result = sqlx::query(
            r#"
               INSERT INTO categories (slug, title)
               VALUES (?, ?)
               "#,
        )
        .bind("rust")
        .bind("Rust")
        .execute(&pool)
        .await
        .expect("category should be inserted");

        let category_id = result.last_insert_rowid();
        let post_repository = SQLitePostRepository::new(pool);
        let post = Post {
            title: "Rust".to_string(),
            slug: "rust".to_string(),
            category: "software development".to_string(),
            categories: Vec::new(),
            tags: Vec::new(),
            draft: false,
            date: Utc::now(),
            excerpt: "".to_string(),
            html: "html content".to_string(),
            raw: "raw content".to_string(),
        };

        let _ = post_repository
            .create(post, category_id)
            .await
            .expect("post should be inserted");

        let posts = post_repository
            .find_all()
            .await
            .expect("posts should be retrieved");

        dbg!(&posts);
        assert_eq!(posts.len(), 1);

        let saved_post = &posts[0];

        assert_eq!(saved_post.slug, "rust");
        assert_eq!(saved_post.title, "Rust");
        assert!(!saved_post.created_at.is_empty());
    }

    #[sqlx::test]
    async fn test_find_by_slug(pool: Pool<Sqlite>) {
        let result = sqlx::query(
            r#"
               INSERT INTO categories (slug, title)
               VALUES (?, ?)
               "#,
        )
        .bind("rust")
        .bind("Rust")
        .execute(&pool)
        .await
        .expect("category should be inserted");

        let category_id = result.last_insert_rowid();
        let post_repository = SQLitePostRepository::new(pool);
        let post = Post {
            title: "Rust".to_string(),
            slug: "rust".to_string(),
            category: "software development".to_string(),
            categories: Vec::new(),
            tags: Vec::new(),
            draft: false,
            date: Utc::now(),
            excerpt: "".to_string(),
            html: "html content".to_string(),
            raw: "raw content".to_string(),
        };

        let _ = post_repository
            .create(post, category_id)
            .await
            .expect("post should be inserted");

        let saved_post = post_repository
            .find_by_slug(&"rust")
            .await
            .expect("post should be retrieved");

        assert_eq!(saved_post.slug, "rust");
        assert_eq!(saved_post.title, "Rust");
        assert!(!saved_post.created_at.is_empty());
    }
}
