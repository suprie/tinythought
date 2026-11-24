use crate::repositories::{AppError, DraftInput, Post, PostDraft, PostRepository, PostTable};
use pulldown_cmark::{Event, Options, Parser, Tag, html};
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

    pub async fn post_by_id(&self, id: i64) -> Result<PostTable, AppError> {
        self.repository.find_by_id(id).await
    }

    pub async fn drafts(&self) -> Result<Vec<PostDraft>, AppError> {
        self.repository.find_drafts(None, None).await
    }

    pub async fn draft(&self, id: i64) -> Result<PostDraft, AppError> {
        self.repository
            .find_drafts(Some(id), None)
            .await?
            .pop()
            .ok_or(AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn draft_for_post(&self, post_id: i64) -> Result<Option<PostDraft>, AppError> {
        Ok(self
            .repository
            .find_drafts(None, Some(post_id))
            .await?
            .pop())
    }

    pub async fn save_draft(
        &self,
        post_id: Option<i64>,
        version: Option<(i64, i64)>,
        input: DraftInput,
    ) -> Result<i64, AppError> {
        self.repository.save_draft(post_id, version, input).await
    }

    pub async fn publish_draft(&self, draft: &PostDraft) -> Result<(), AppError> {
        let html = self.render_markdown(&draft.raw);
        self.repository
            .publish_draft(draft, &slugify(&draft.title), &draft.raw, &html)
            .await
    }

    pub async fn discard_draft(&self, id: i64) -> Result<(), AppError> {
        self.repository.discard_draft(id).await
    }

    pub async fn image(
        &self,
        draft_id: Option<i64>,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, AppError> {
        self.repository.find_image(draft_id, filename).await
    }

    pub async fn create_new_post(&self, post: &Post, category_id: i64) -> Result<(), AppError> {
        let html = self.render_markdown(&post.raw);
        let mut clone_post = post.clone();
        clone_post.html = html;
        self.repository.create(clone_post, category_id).await
    }

    pub async fn delete_post(&self, id: i64) -> Result<(), AppError> {
        self.repository.delete(id).await
    }

    pub fn render_markdown(&self, body: &str) -> String {
        let mut html = String::new();
        html::push_html(
            &mut html,
            Parser::new_ext(body, Options::ENABLE_STRIKETHROUGH),
        );
        html
    }

    pub fn render_draft(&self, draft: &PostDraft) -> String {
        let parser = Parser::new_ext(&draft.raw, Options::ENABLE_STRIKETHROUGH).map(|mut event| {
            if let Event::Start(Tag::Image { dest_url, .. }) = &mut event
                && let Some(filename) = dest_url.strip_prefix("/post-images/")
                && draft.image_filenames.iter().any(|image| image == filename)
            {
                *dest_url = format!("/admin/drafts/{}/image/{filename}", draft.id).into();
            }
            event
        });
        let mut html = String::new();
        html::push_html(&mut html, parser);
        html
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if (ch == ' ' || ch == '-' || ch == '_') && !slug.is_empty() && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}
