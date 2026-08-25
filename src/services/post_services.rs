use crate::repositories::{AppError, PostRepository, PostTable};
use std::sync::Arc;

pub struct PostService {
    repository: Arc<dyn PostRepository>,
}

impl PostService {
    pub fn new(repository: Arc<dyn PostRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_posts(&self) -> Result<Vec<PostTable>, AppError> {
        self.repository.find_all().await
    }

    pub async fn post_by_slug(&self, slug: &str) -> Result<PostTable, AppError> {
        self.repository.find_by_slug(slug).await
    }
}
