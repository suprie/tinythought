use crate::repositories::{
    DraftInput, Post, PostDraft,
    models::{AppError, PostTable},
};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;

#[async_trait]
pub trait PostRepository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<PostTable>, AppError>;
    async fn find_by_slug(&self, slug: &str) -> Result<PostTable, AppError>;
    async fn find_by_id(&self, id: i64) -> Result<PostTable, AppError>;

    async fn create(&self, post: Post, category_id: i64) -> Result<(), AppError>;
    async fn delete(&self, id: i64) -> Result<(), AppError>;

    async fn find_drafts(
        &self,
        id: Option<i64>,
        post_id: Option<i64>,
    ) -> Result<Vec<PostDraft>, AppError>;
    async fn save_draft(
        &self,
        post_id: Option<i64>,
        version: Option<(i64, i64)>,
        input: DraftInput,
    ) -> Result<i64, AppError>;
    async fn publish_draft(
        &self,
        draft: &PostDraft,
        slug: &str,
        raw: &str,
        html: &str,
    ) -> Result<(), AppError>;
    async fn discard_draft(&self, id: i64) -> Result<(), AppError>;
    async fn find_image(
        &self,
        draft_id: Option<i64>,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, AppError>;
}

pub struct PgPostRepository {
    pool: PgPool,
}

impl PgPostRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PostRepository for PgPostRepository {
    async fn find_all(&self) -> Result<Vec<PostTable>, AppError> {
        sqlx::query_as!(
            PostTable,
            r#"
            Select
                p.id,
                p.title,
                p.slug,
                c.id as category_id,
                c.title as category_name,
                c.slug as category_slug,
                p.body_markdown as raw,
                p.body_html as html,
                p.created_at as created_at,
                p.published_date
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
            r#"INSERT INTO posts (title, slug, category_id, body_html, body_markdown, updated_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
            &post.title,
            &post.slug,
            &category_id,
            &post.html,
            &post.raw,
            Utc::now(),
            Utc::now()
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
                c.id as category_id,
                c.title as category_name,
                c.slug as category_slug,
                p.body_markdown as raw,
                p.body_html as html,
                p.created_at as created_at,
                p.published_date
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

    async fn find_by_id(&self, id: i64) -> Result<PostTable, AppError> {
        sqlx::query_as(
            "SELECT p.id, p.title, p.slug, p.category_id, c.title AS category_name,
                    c.slug AS category_slug, p.body_markdown AS raw, p.body_html AS html,
                    p.created_at, p.published_date
             FROM posts p JOIN categories c ON c.id = p.category_id WHERE p.id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)
    }

    async fn delete(&self, id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM posts WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(AppError::Database)
            .map(|_| ())
    }

    async fn find_drafts(
        &self,
        id: Option<i64>,
        post_id: Option<i64>,
    ) -> Result<Vec<PostDraft>, AppError> {
        sqlx::query_as(
            "SELECT d.id, d.post_id, d.title, d.category, d.body_markdown AS raw, d.revision,
                    ARRAY(SELECT i.filename FROM post_images i WHERE i.draft_id = d.id) AS image_filenames,
                    d.published_date,
                    COALESCE(p.published_date, (p.created_at AT TIME ZONE 'UTC')::date)
                        AS live_published_date
             FROM post_drafts d
             LEFT JOIN posts p ON p.id = d.post_id
             WHERE ($1::bigint IS NULL OR d.id = $1) AND ($2::bigint IS NULL OR d.post_id = $2)
             ORDER BY d.id DESC",
        )
        .bind(id)
        .bind(post_id)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::Database)
    }

    async fn save_draft(
        &self,
        post_id: Option<i64>,
        version: Option<(i64, i64)>,
        input: DraftInput,
    ) -> Result<i64, AppError> {
        let mut tx = self.pool.begin().await.map_err(AppError::Database)?;
        let id = match version {
            Some((id, revision)) => {
                sqlx::query_scalar(
                    "UPDATE post_drafts SET title = $1, category = $2, body_markdown = $3,
                            published_date = $4, revision = revision + 1
                     WHERE id = $5 AND revision = $6 RETURNING id",
                )
                .bind(input.title)
                .bind(input.category)
                .bind(input.raw)
                .bind(input.published_date)
                .bind(id)
                .bind(revision)
                .fetch_one(&mut *tx)
                .await
            }
            None => sqlx::query_scalar(
                "INSERT INTO post_drafts (title, category, body_markdown, published_date, post_id)
                     VALUES ($1, $2, $3, $4, $5) RETURNING id",
            )
            .bind(input.title)
            .bind(input.category)
            .bind(input.raw)
            .bind(input.published_date)
            .bind(post_id)
            .fetch_one(&mut *tx)
            .await,
        }
        .map_err(AppError::Database)?;

        sqlx::query("DELETE FROM post_images WHERE draft_id = $1 AND NOT (filename = ANY($2))")
            .bind(id)
            .bind(input.referenced_images)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
        for (filename, bytes) in input.images {
            sqlx::query("INSERT INTO post_images (filename, draft_id, bytes) VALUES ($1, $2, $3)")
                .bind(filename)
                .bind(id)
                .bind(bytes)
                .execute(&mut *tx)
                .await
                .map_err(AppError::Database)?;
        }
        tx.commit().await.map_err(AppError::Database)?;
        Ok(id)
    }

    async fn publish_draft(
        &self,
        draft: &PostDraft,
        slug: &str,
        raw: &str,
        html: &str,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(AppError::Database)?;
        // Lock the live post first, matching the lock order of its cascading delete.
        if let Some(post_id) = draft.post_id {
            sqlx::query("SELECT id FROM posts WHERE id = $1 FOR UPDATE")
                .bind(post_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(AppError::Database)?;
        }
        sqlx::query("SELECT id FROM post_drafts WHERE id = $1 AND revision = $2 FOR UPDATE")
            .bind(draft.id)
            .bind(draft.revision)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?;

        let existing_category = sqlx::query_scalar(
            "SELECT c.id FROM posts p JOIN categories c ON c.id = p.category_id
             WHERE p.id = $1 AND c.title = $2",
        )
        .bind(draft.post_id)
        .bind(&draft.category)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::Database)?;
        let category_id: i64 = match existing_category {
            Some(id) => id,
            None => sqlx::query_scalar(
                "INSERT INTO categories (slug, title) VALUES ($1, $2)
                     ON CONFLICT (slug) DO UPDATE SET slug = EXCLUDED.slug RETURNING id",
            )
            .bind(draft.category.to_lowercase().replace(' ', "-"))
            .bind(&draft.category)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?,
        };

        if let Some(post_id) = draft.post_id {
            sqlx::query(
                "UPDATE posts SET title = $1, category_id = $2, body_markdown = $3, body_html = $4,
                        published_date = COALESCE($5, published_date), updated_at = now() WHERE id = $6",
            )
            .bind(&draft.title)
            .bind(category_id)
            .bind(raw)
            .bind(html)
            .bind(draft.published_date)
            .bind(post_id)
            .execute(&mut *tx)
            .await
        } else {
            sqlx::query(
                "INSERT INTO posts (title, category_id, body_markdown, body_html, slug, published_date)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&draft.title)
            .bind(category_id)
            .bind(raw)
            .bind(html)
            .bind(slug)
            .bind(draft.published_date)
            .execute(&mut *tx)
            .await
        }
        .map_err(AppError::Database)?;

        sqlx::query("UPDATE post_images SET draft_id = NULL WHERE draft_id = $1")
            .bind(draft.id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
        sqlx::query("DELETE FROM post_drafts WHERE id = $1")
            .bind(draft.id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::Database)?;
        tx.commit().await.map_err(AppError::Database)
    }

    async fn discard_draft(&self, id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM post_drafts WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(AppError::Database)
    }

    async fn find_image(
        &self,
        draft_id: Option<i64>,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        sqlx::query_scalar(
            "SELECT bytes FROM post_images WHERE filename = $1 AND draft_id IS NOT DISTINCT FROM $2",
        )
        .bind(filename)
        .bind(draft_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[sqlx::test]
    async fn test_find_all(pool: PgPool) {
        let category_id: i64 = sqlx::query_scalar(
            r#"
               INSERT INTO categories (slug, title)
               VALUES ($1, $2)
               RETURNING id
               "#,
        )
        .bind("rust")
        .bind("Rust")
        .fetch_one(&pool)
        .await
        .expect("category should be inserted");
        let post_repository = PgPostRepository::new(pool);
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
        assert!(saved_post.created_at <= Utc::now());
    }

    #[sqlx::test]
    async fn test_find_by_slug(pool: PgPool) {
        let category_id: i64 = sqlx::query_scalar(
            r#"
               INSERT INTO categories (slug, title)
               VALUES ($1, $2)
               RETURNING id
               "#,
        )
        .bind("rust")
        .bind("Rust")
        .fetch_one(&pool)
        .await
        .expect("category should be inserted");
        let post_repository = PgPostRepository::new(pool);
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
        assert!(saved_post.created_at <= Utc::now());
    }
}
