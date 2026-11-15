use crate::repositories::{AppError, Post, PostRepository, PostTable};
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

    pub async fn create_new_post(&self, post: &Post, category_id: i64) -> Result<(), AppError> {
        let html = self.render_markdown(&post.raw);
        let mut clone_post = post.clone();
        clone_post.html = html;
        self.repository.create(clone_post, category_id).await
    }

    fn render_markdown(&self, body: &str) -> String {
        let parser =
            pulldown_cmark::Parser::new_ext(body, pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        html
    }
}
