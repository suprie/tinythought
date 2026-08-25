-- Add migration script here

-- 3. Create a new table with NOT NULL constraint
CREATE TABLE posts_new (
    id INTEGER PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    category_id INT NOT NULL,
    body_markdown TEXT NOT NULL,
    body_html TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- 4. Copy data from old table
INSERT INTO posts_new (id, title, slug, category_id, body_markdown, body_html, updated_at, created_at)
SELECT id, title, slug, category_id, body_markdown, body_html, updated_at, created_at FROM posts;

-- 5. Drop old table
DROP TABLE posts;

-- 6. Rename new table
ALTER TABLE posts_new RENAME TO posts;
