use std::collections::HashMap;
use std::path::Path;
use std::{format, fs};

use chrono::{DateTime, Utc};

use crate::repositories::{Category, Frontmatter, Post};

/// Snapshot of every published post, ready to serve. Rebuilt wholesale on
/// (re)index and swapped in atomically — never mutated in place.
#[derive(Debug, Clone, Default)]
#[deprecated(since = "0.1.0", note = "moving on SQLite")]
pub struct Index {
    pub categories: Vec<Category>,
    pub all_posts: Vec<Post>,
    pub posts_by_category: HashMap<String, Vec<Post>>,
    pub posts_by_slug: HashMap<String, Post>,
}

impl Index {
    pub fn category(&self, slug: &str) -> Option<&Category> {
        self.categories.iter().find(|c| c.slug == slug)
    }
}

/// Walk `content_dir` (expected: `public/content/<category>/<post>.md`) and
/// build a fresh index. Malformed posts are skipped with a warning rather
/// than failing the whole index — one bad file shouldn't take the site down.
#[deprecated(since = "0.1.0", note = "moving on SQLite")]
pub fn build_index(content_dir: &Path) -> Index {
    let mut all_posts = Vec::new();
    let mut posts_by_category: HashMap<String, Vec<Post>> = HashMap::new();

    let Ok(category_dirs) = fs::read_dir(content_dir) else {
        eprintln!("content dir not found: {}", content_dir.display());
        return Index::default();
    };

    let mut category_dirs: Vec<_> = category_dirs.filter_map(|e| e.ok()).collect();
    category_dirs.sort_by_key(|e| e.file_name());

    for category_entry in category_dirs {
        let path = category_entry.path();
        if !path.is_dir() {
            continue;
        }
        let category_slug = category_entry.file_name().to_string_lossy().to_string();

        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        let mut files: Vec<_> = files.filter_map(|e| e.ok()).collect();
        files.sort_by_key(|e| e.file_name());

        for file_entry in files {
            let file_path = file_entry.path();
            if file_path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            match parse_post(&file_path, &category_slug) {
                Ok(post) if post.draft => {
                    // Drafts are indexed nowhere: not listed, not routable.
                }
                Ok(post) => {
                    posts_by_category
                        .entry(post.category.clone())
                        .or_default()
                        .push(post.clone());
                    all_posts.push(post);
                }
                Err(err) => {
                    eprintln!("skipping {}: {err}", file_path.display());
                }
            }
        }
    }

    all_posts.sort_by(|a, b| b.date.cmp(&a.date));
    for posts in posts_by_category.values_mut() {
        posts.sort_by(|a, b| b.date.cmp(&a.date));
    }

    let categories = posts_by_category
        .iter()
        .map(|(slug, posts)| Category::from_slug(slug, posts.len()))
        .collect::<Vec<_>>();
    let mut categories = categories;
    categories.sort_by(|a, b| a.name.cmp(&b.name));

    let posts_by_slug = all_posts
        .iter()
        .cloned()
        .map(|p| (p.slug.clone(), p))
        .collect();

    Index {
        categories,
        all_posts,
        posts_by_category,
        posts_by_slug,
    }
}

fn parse_post(path: &Path, category_slug: &str) -> Result<Post, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (frontmatter, body) = split_frontmatter(&raw)?;
    let fm: Frontmatter = serde_yaml::from_str(frontmatter).map_err(|e| e.to_string())?;

    let slug = fm.slug.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });
    let categories = if fm.categories.is_empty() {
        vec![category_slug.to_string()]
    } else {
        fm.categories
    };

    let modified = fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    let date: DateTime<Utc> = modified.into();

    let html = render_markdown(body.trim());
    let excerpt = plain_excerpt(body.trim(), 180);

    Ok(Post {
        title: fm.title,
        slug,
        category: category_slug.to_string(),
        categories,
        tags: fm.tags,
        draft: fm.draft,
        date,
        excerpt,
        html,
        raw: body.trim().to_string(),
    })
}

/// Splits a file into its `---`-delimited YAML frontmatter and the
/// remaining markdown body.
fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw); // tolerate a BOM
    let rest = raw
        .strip_prefix("---")
        .ok_or("missing frontmatter (file must start with `---`)")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
        .unwrap_or(rest);

    let end = rest
        .find("\n---")
        .ok_or("missing closing `---` for frontmatter")?;
    let frontmatter = &rest[..end];
    let body = &rest[end + 4..];
    let body = body
        .strip_prefix('\n')
        .or_else(|| body.strip_prefix("\r\n"))
        .unwrap_or(body);
    Ok((frontmatter, body))
}

fn render_markdown(body: &str) -> String {
    let parser =
        pulldown_cmark::Parser::new_ext(body, pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

/// Plain-text excerpt for list views: markdown stripped, truncated to
/// roughly `max_chars`, cut on a word boundary.
pub(crate) fn plain_excerpt(body: &str, max_chars: usize) -> String {
    use pulldown_cmark::{Event, Parser};

    let mut text = String::new();
    for event in Parser::new(body) {
        match event {
            Event::Text(t) | Event::Code(t) => text.push_str(&t),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            Event::End(pulldown_cmark::TagEnd::Paragraph) if !text.is_empty() => break,
            _ => {}
        }
    }
    let text = text.trim();

    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    match truncated.rsplit_once(' ') {
        Some((head, _)) => format!("{head}…"),
        None => format!("{truncated}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_body_renders_to_html() {
        let result = render_markdown("---\ntitle: Md\n---\n**bold** and _em_\n");

        assert!(result.contains("<strong>bold</strong>"));
        assert!(result.contains("<em>em</em>"));
    }
}
