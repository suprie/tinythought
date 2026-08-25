CREATE TABLE IF NOT EXISTS posts (
  id INTEGER PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  category TEXT NOT NULL,
  body_markdown TEXT NOT NULL,
  body_html TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  created_at TEXT DEFAULT CURRENT_TIMESTAMP
)-- Add migration script here
