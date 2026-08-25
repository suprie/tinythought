use crate::repositories::{AppError, CategoryRepository, CategoryTable};
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
}
