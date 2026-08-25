use crate::repositories::{
    AppError, Category, CategoryRepository, CategoryTable, Post, PostRepository,
};
use std::sync::Arc;

pub struct MigrationService {
    category_repository: Arc<dyn CategoryRepository>,
    post_repository: Arc<dyn PostRepository>,
}

impl MigrationService {
    pub fn new(
        category_repository: Arc<dyn CategoryRepository>,
        post_repository: Arc<dyn PostRepository>,
    ) -> Self {
        Self {
            category_repository,
            post_repository,
        }
    }

    pub async fn migrate(&self, categories: &Vec<Category>, posts: &Vec<Post>) {
        for category in categories {
            let _ = self.category_repository.create(category.clone()).await;
        }

        for post in posts {
            let category = self
                .find_or_create_category(&post.category)
                .await
                .expect("find and create should be succeed");
            let _ = self.post_repository.create(post.clone(), category.id).await;
        }
    }

    async fn find_or_create_category(&self, category: &str) -> Result<CategoryTable, AppError> {
        let category_slug = category.to_lowercase().replace(" ", "-");
        if let Some(category_table) = self
            .category_repository
            .find_by_slug(&category_slug)
            .await?
        {
            return Ok(category_table);
        }

        let category = Category {
            name: category.to_string(),
            slug: category_slug,
            post_count: 0,
        };

        self.category_repository.create(category).await
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use chrono::Utc;
    use sqlx::Sqlite;
    use sqlx::pool::Pool;

    use crate::repositories::{Category, Post, SQLiteCategoryRepository, SQLitePostRepository};

    #[sqlx::test]
    async fn test_migrate(pool: Pool<Sqlite>) {
        let category_repository = Arc::new(SQLiteCategoryRepository::new(pool.clone()));
        let post_repository = Arc::new(SQLitePostRepository::new(pool));

        let migration_services =
            MigrationService::new(category_repository.clone(), post_repository.clone());

        let category = Category {
            name: "Test With spaces".to_string(),
            slug: "test-with-spaces".to_string(),
            post_count: 2,
        };

        let post = Post {
            title: "Post".to_string(),
            slug: "post".to_string(),
            category: "test-with-spaces".to_string(),
            categories: Vec::new(),
            tags: Vec::new(),
            draft: false,
            date: Utc::now(),
            excerpt: "this is excerpt".to_string(),
            html: "this is html".to_string(),
            raw: "this is raw".to_string(),
        };

        let categories = vec![category];
        let posts = vec![post];

        migration_services.migrate(&categories, &posts).await;

        let saved_categories = Arc::clone(&category_repository)
            .find_all()
            .await
            .expect("categories should be able to queried");
        assert_eq!(saved_categories.len(), 1);

        let saved_posts = Arc::clone(&post_repository)
            .find_all()
            .await
            .expect("posts should be able to queried");
        assert_eq!(saved_posts.len(), 1);

        let saved_post = saved_posts[0].clone();
        assert_eq!(saved_post.title, "Post");
        assert_eq!(saved_post.slug, "post");
        assert_eq!(saved_post.category_id, saved_categories[0].id);
    }
}

// Baca dari categories dulu dari current state
// Masukin ke database
// Baca posts dari index
// Masukin ke database
// Punya dependencies ke categories sama posts
