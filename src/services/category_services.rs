use crate::repositories::{AppError, Category, CategoryRepository, CategoryTable};
use std::sync::Arc;

pub struct CategoryService {
    repository: Arc<dyn CategoryRepository>,
}

impl CategoryService {
    pub fn new(repository: Arc<dyn CategoryRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_categories(&self) -> Result<Vec<CategoryTable>, AppError> {
        self.repository.find_all().await
    }

    pub async fn get_categories_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<CategoryTable>, AppError> {
        self.repository.find_by_slug(slug).await
    }

    pub async fn create_category(&self, category: &Category) -> Result<CategoryTable, AppError> {
        self.repository.create(category.clone()).await
    }

    pub async fn find_or_create_category(&self, category: &str) -> Result<CategoryTable, AppError> {
        let category_slug = category.to_lowercase().replace(" ", "-");
        if let Some(category_table) = self.repository.find_by_slug(&category_slug).await? {
            return Ok(category_table);
        }

        let category = Category {
            name: category.to_string(),
            slug: category_slug,
            post_count: 0,
        };

        self.repository.create(category).await
    }
}
