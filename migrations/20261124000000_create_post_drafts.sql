CREATE TABLE post_drafts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    post_id BIGINT UNIQUE REFERENCES posts (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    category TEXT NOT NULL,
    body_markdown TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1
);

CREATE TABLE post_images (
    filename TEXT PRIMARY KEY,
    draft_id BIGINT UNIQUE REFERENCES post_drafts (id) ON DELETE CASCADE,
    bytes BYTEA NOT NULL
);
