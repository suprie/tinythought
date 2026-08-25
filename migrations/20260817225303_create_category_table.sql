PRAGMA foreign_keys = ON;
-- Add migration script here
CREATE TABLE IF NOT EXISTS categories (
  id INTEGER PRIMARY KEY NOT NULL,
  title TEXT NOT NULL
);

ALTER TABLE posts ADD COLUMN category_id INTEGER REFERENCES categories(id);
ALTER TABLE posts DROP COLUMN category;
