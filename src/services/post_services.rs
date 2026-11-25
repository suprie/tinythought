use crate::repositories::{AppError, DraftInput, Post, PostDraft, PostRepository, PostTable};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use std::sync::{Arc, LazyLock};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;

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
        let html = crate::content::render_markdown(&draft.raw);
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
        let html = crate::content::render_markdown(&post.raw);
        let mut clone_post = post.clone();
        clone_post.html = html;
        self.repository.create(clone_post, category_id).await
    }

    pub async fn delete_post(&self, id: i64) -> Result<(), AppError> {
        self.repository.delete(id).await
    }

    pub fn render_markdown(&self, body: &str) -> String {
        render_events(Parser::new_ext(body, Options::ENABLE_STRIKETHROUGH))
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
        render_events(parser)
    }
}

fn render_events<'a>(mut events: impl Iterator<Item = Event<'a>>) -> String {
    let mut output = String::new();
    html::push_html(
        &mut output,
        std::iter::from_fn(|| {
            let event = events.next()?;
            let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = event else {
                return Some(event);
            };
            let mut code = String::new();
            for event in events.by_ref() {
                match event {
                    Event::Text(text) => code.push_str(&text),
                    Event::End(TagEnd::CodeBlock) => break,
                    _ => {}
                }
            }

            let language = info.split_whitespace().next().unwrap_or_default();
            let block = highlight_code(language, &code).unwrap_or_else(|| {
                let mut plain = String::new();
                html::push_html(
                    &mut plain,
                    [
                        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))),
                        Event::Text(code.into()),
                        Event::End(TagEnd::CodeBlock),
                    ]
                    .into_iter(),
                );
                plain
            });
            Some(Event::Html(block.into()))
        }),
    );
    output
}

fn highlight_code(language: &str, code: &str) -> Option<String> {
    if language.is_empty()
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-#.".contains(&byte))
        || matches!(
            language.to_ascii_lowercase().as_str(),
            "text" | "txt" | "plain" | "plaintext"
        )
    {
        return None;
    }

    static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
    let syntax = SYNTAXES.find_syntax_by_token(language)?;
    let mut highlighter = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        &SYNTAXES,
        ClassStyle::SpacedPrefixed { prefix: "syntax-" },
    );
    for line in code.split_inclusive('\n') {
        highlighter
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    let highlighted = highlighter.finalize();
    Some(format!(
        "<pre><code class=\"language-{language}\">{highlighted}</code></pre>\n"
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_the_example_posts_languages_and_leaves_plain_code_escaped() {
        let example =
            include_str!("../../public/content/software-development/idempotency-mobile.md");
        let html = render_events(Parser::new(example));
        for language in ["swift", "bash", "json"] {
            let start = format!("<code class=\"language-{language}\">");
            let (_, rest) = html.split_once(&start).unwrap();
            let (block, _) = rest.split_once("</code>").unwrap();
            assert!(
                [
                    "keyword", "storage", "string", "constant", "variable", "entity", "support"
                ]
                .iter()
                .any(|scope| block.contains(&format!("syntax-{scope}"))),
                "{language} block has no styled tokens: {block}"
            );
        }

        let markdown = "```swift\nlet value = \"<script>&\"\n```\n\n```plaintext\n<script>&\n```\n\n```unknown\n<script>&\n```\n\n```\n<script>&\n```";
        let html = render_events(Parser::new(markdown));
        assert!(!html.contains("<script>"));
        for class in [
            " class=\"language-plaintext\"",
            " class=\"language-unknown\"",
            "",
        ] {
            assert!(html.contains(&format!("<code{class}>&lt;script&gt;&amp;\n</code>")));
        }
    }
}
