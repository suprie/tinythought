# Mini Blog

Tiny, database-free blog. Rust + Axum + Askama. Posts are markdown files with
YAML frontmatter — no admin, no DB, just files.

## Run

```sh
cargo run
```

Server listens on `http://localhost:3000`. Indexes `public/content/` at
startup, then reindexes every 5 hours.

Set `SITE_URL` (e.g. `https://yourdomain.com`, no trailing slash) in
production — it's the origin used for canonical links, Open Graph/Twitter
card URLs, and `/sitemap.xml`. Defaults to `http://localhost:3000`.

## Test

```sh
cargo test
```

## Content structure

```
public/content/<category-slug>/<article-slug>.md
```

Each subdirectory of `public/content` is a category (auto-populated — no
config needed). Each `.md` file inside it is a post.

```markdown
---
title: "#ACID"
slug: acid
categories: ["software-development"]
tags: ["databases", "fundamentals"]
draft: false
---
Post body in markdown.
```

- `title` — required.
- `slug` — optional, defaults to the filename (used in `/posts/<slug>`).
- `categories` — optional, defaults to `[<folder-name>]`. The folder always
  decides which `/<category-slug>` page a post lists under.
- `tags` — optional, shown on the post page.
- `draft` — optional, defaults to `false`. `true` removes the post from
  every listing and 404s its `/posts/<slug>` page.
- Published date isn't a frontmatter field — it's the file's last-modified
  time on disk.

## Routes

- `/` — all categories + all published posts
- `/<category-slug>` — posts in one category
- `/posts/<article-slug>` — a single post
- `/static/*` — CSS, served from `public/static`
- `/sitemap.xml` — every category + published post, for search engines
- `/robots.txt` — points crawlers at the sitemap

## Layout

```
src/
  models.rs   Post, Category, Frontmatter
  content.rs  indexing: scan, parse frontmatter/markdown, build Index
  views.rs    Askama template structs
  routes.rs   Axum handlers
  lib.rs      state, router, reindex loop
  main.rs     entrypoint
templates/    Askama .html templates
public/       static assets + content/
tests/        integration tests (tower::oneshot against the real router)
```
