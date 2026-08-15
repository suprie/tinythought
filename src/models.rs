use serde::Deserialize;

/// Raw fields parsed straight out of a post's YAML frontmatter block.
#[derive(Debug, Deserialize, Default)]
pub struct Frontmatter {
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub draft: bool,
}

/// A fully parsed, render-ready post.
#[derive(Debug, Clone)]
pub struct Post {
    pub title: String,
    pub slug: String,
    /// Canonical category, derived from the folder the post lives in.
    pub category: String,
    /// Categories declared in frontmatter (defaults to `[category]`).
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub draft: bool,
    /// Last-modified time of the source file — doubles as the "published" date.
    pub date: chrono::DateTime<chrono::Utc>,
    pub excerpt: String,
    pub html: String,
}

impl Post {
    pub fn date_label(&self) -> String {
        self.date.format("%b %-d, %Y").to_string()
    }
}

/// A category, auto-populated from a subdirectory of `public/content`.
#[derive(Debug, Clone)]
pub struct Category {
    pub slug: String,
    pub name: String,
    pub post_count: usize,
}

impl Category {
    pub fn from_slug(slug: &str, post_count: usize) -> Self {
        let name = slug
            .split(['-', '_'])
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        Category {
            slug: slug.to_string(),
            name,
            post_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_name_is_derived_from_slug() {
        assert_eq!(Category::from_slug("life-lessons", 2).name, "Life Lessons");
        assert_eq!(
            Category::from_slug("software-development", 5).name,
            "Software Development"
        );
        assert_eq!(Category::from_slug("misc", 0).name, "Misc");
    }
}
