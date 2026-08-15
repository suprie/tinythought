use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::models::{Category, Frontmatter, Post};

/// Snapshot of every published post, ready to serve. Rebuilt wholesale on
/// (re)index and swapped in atomically — never mutated in place.
#[derive(Debug, Clone, Default)]
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

    let modified = fs::metadata(path).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
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
    })
}

/// Splits a file into its `---`-delimited YAML frontmatter and the
/// remaining markdown body.
fn split_frontmatter(raw: &str) -> Result<(&str, &str), String> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw); // tolerate a BOM
    let rest = raw
        .strip_prefix("---")
        .ok_or("missing frontmatter (file must start with `---`)")?;
    let rest = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n")).unwrap_or(rest);

    let end = rest
        .find("\n---")
        .ok_or("missing closing `---` for frontmatter")?;
    let frontmatter = &rest[..end];
    let body = &rest[end + 4..];
    let body = body.strip_prefix('\n').or_else(|| body.strip_prefix("\r\n")).unwrap_or(body);
    Ok((frontmatter, body))
}

fn render_markdown(body: &str) -> String {
    let parser = pulldown_cmark::Parser::new_ext(body, pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

/// Plain-text excerpt for list views: markdown stripped, truncated to
/// roughly `max_chars`, cut on a word boundary.
fn plain_excerpt(body: &str, max_chars: usize) -> String {
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
    use std::io::Write;
    use tempfile_helper::TempDir;

    mod tempfile_helper {
        use std::path::{Path, PathBuf};

        /// Minimal self-cleaning temp directory — avoids pulling in the
        /// `tempfile` crate for a handful of tests.
        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new(name: &str) -> Self {
                let dir = std::env::temp_dir().join(format!(
                    "miniblog-test-{name}-{}",
                    std::process::id()
                ));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                TempDir(dir)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn write_post(dir: &Path, category: &str, filename: &str, contents: &str) {
        let cat_dir = dir.join(category);
        std::fs::create_dir_all(&cat_dir).unwrap();
        let mut f = std::fs::File::create(cat_dir.join(filename)).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn split_frontmatter_separates_yaml_and_body() {
        let raw = "---\ntitle: Hi\n---\nBody text\n";
        let (fm, body) = split_frontmatter(raw).unwrap();
        assert_eq!(fm, "title: Hi");
        assert_eq!(body, "Body text\n");
    }

    #[test]
    fn split_frontmatter_errors_without_leading_marker() {
        assert!(split_frontmatter("no frontmatter here").is_err());
    }

    #[test]
    fn build_index_skips_drafts_and_derives_category_from_folder() {
        let tmp = TempDir::new("index-basic");
        write_post(
            tmp.path(),
            "software-development",
            "acid.md",
            "---\ntitle: \"#ACID\"\nslug: acid\ntags: [databases]\ndraft: false\n---\nAtomicity, Consistency, Isolation, Durability.\n",
        );
        write_post(
            tmp.path(),
            "software-development",
            "hidden.md",
            "---\ntitle: Hidden\ndraft: true\n---\nShould not appear.\n",
        );
        write_post(
            tmp.path(),
            "life-lessons",
            "less.md",
            "---\ntitle: \"#Less\"\n---\nRemove what does not matter.\n",
        );

        let index = build_index(tmp.path());

        assert_eq!(index.all_posts.len(), 2);
        assert_eq!(index.categories.len(), 2);
        assert!(index.posts_by_slug.contains_key("acid"));
        assert!(!index.posts_by_slug.contains_key("hidden"));

        let sd_posts = &index.posts_by_category["software-development"];
        assert_eq!(sd_posts.len(), 1);
        assert_eq!(sd_posts[0].category, "software-development");
        assert_eq!(sd_posts[0].categories, vec!["software-development"]);

        let less = &index.posts_by_slug["less"];
        assert_eq!(less.slug, "less"); // derived from filename, no explicit slug
    }

    #[test]
    fn build_index_skips_malformed_post_without_failing() {
        let tmp = TempDir::new("index-malformed");
        write_post(tmp.path(), "cat", "bad.md", "no frontmatter at all");
        write_post(
            tmp.path(),
            "cat",
            "good.md",
            "---\ntitle: Good\n---\nFine.\n",
        );

        let index = build_index(tmp.path());
        assert_eq!(index.all_posts.len(), 1);
        assert_eq!(index.all_posts[0].title, "Good");
    }

    #[test]
    fn markdown_body_renders_to_html() {
        let tmp = TempDir::new("index-markdown");
        write_post(
            tmp.path(),
            "cat",
            "post.md",
            "---\ntitle: Md\n---\n**bold** and _em_\n",
        );
        let index = build_index(tmp.path());
        let post = &index.all_posts[0];
        assert!(post.html.contains("<strong>bold</strong>"));
        assert!(post.html.contains("<em>em</em>"));
    }

    #[test]
    fn excerpt_stops_at_max_chars_on_a_word_boundary() {
        let long = "word ".repeat(100);
        let excerpt = plain_excerpt(&long, 20);
        assert!(excerpt.chars().count() <= 21); // + ellipsis
        assert!(excerpt.ends_with('…'));
    }
}
