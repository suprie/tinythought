use crate::repositories::models::AppError;
use crate::repositories::{Category, CategoryTable};
use async_trait::async_trait;
use sqlx::pool::Pool;
use sqlx::{Sqlite, SqlitePool};

#[async_trait]
pub trait CategoryRepository: Send + Sync {
    async fn find_all(&self) -> Result<Vec<CategoryTable>, AppError>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<CategoryTable>, AppError>;
    async fn create(&self, category: Category) -> Result<CategoryTable, AppError>;
}

pub struct SQLiteCategoryRepository {
    pool: Pool<Sqlite>,
}

impl SQLiteCategoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CategoryRepository for SQLiteCategoryRepository {
    async fn find_all(&self) -> Result<Vec<CategoryTable>, AppError> {
        sqlx::query_as!(CategoryTable, "Select * FROM categories")
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::Database)
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<CategoryTable>, AppError> {
        sqlx::query_as!(
            CategoryTable,
            "Select * FROM categories WHERE slug = $1",
            &slug
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::Database)
    }

    async fn create(&self, category: Category) -> Result<CategoryTable, AppError> {
        sqlx::query_as!(
            CategoryTable,
            "INSERT INTO categories (slug, title) VALUES ($1, $2) RETURNING id, title, slug",
            &category.slug,
            &category.name
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::Database)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::Category;

    #[sqlx::test]
    async fn test_find_all(pool: Pool<Sqlite>) {
        let category_repository = SQLiteCategoryRepository::new(pool);
        let category = Category {
            slug: "slug".to_string(),
            name: "test".to_string(),
            post_count: 0,
        };
        let _ = category_repository.create(category).await;
        let categories = category_repository
            .find_all()
            .await
            .expect("categories should be retrieved");

        assert_eq!(categories.len(), 1);

        let saved_categories = &categories[0];

        assert_eq!(saved_categories.slug, "slug");
        assert_eq!(saved_categories.title, "test");
    }

    #[sqlx::test]
    async fn test_insert(pool: Pool<Sqlite>) {
        let category_repository = SQLiteCategoryRepository::new(pool);
        let category = Category {
            slug: "slug".to_string(),
            name: "test".to_string(),
            post_count: 0,
        };
        let inserted_object = category_repository
            .create(category)
            .await
            .expect("Should return categoryTable");

        assert_eq!(inserted_object.slug, "slug");
        assert_eq!(inserted_object.title, "test");
    }

    #[sqlx::test]
    async fn test_find_by_slug(pool: Pool<Sqlite>) {
        let _ = sqlx::query(
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

        let category_repository = SQLiteCategoryRepository::new(pool);
        if let Some(result) = category_repository
            .find_by_slug("rust")
            .await
            .expect("query should succeed")
        {
            assert_eq!(result.slug, "rust");
            assert_eq!(result.title, "Rust");
        }
    }

    #[sqlx::test]
    async fn test_find_by_slug_found_nothing(pool: Pool<Sqlite>) {
        let category_repository = SQLiteCategoryRepository::new(pool);
        let result = category_repository
            .find_by_slug("non-existing")
            .await
            .expect("should be query non existing slug");

        assert!(result.is_none());
    }
}
