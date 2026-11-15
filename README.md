# tinythought

Tiny blog. Rust + Axum + Askama + PostgreSQL. Posts are markdown files with
YAML frontmatter; the database stores the published copy that the site serves.

## Run locally

Start a PostgreSQL instance and point `DATABASE_URL` at it:

```sh
docker run --rm -d --name tinythought-pg \
  -e POSTGRES_USER=tinythought \
  -e POSTGRES_PASSWORD=tinythought \
  -e POSTGRES_DB=tinythought \
  -p 5432:5432 postgres:16-alpine

cp .env.example .env
# edit AUTH_TOKEN and DATABASE_URL if needed
cargo run
```

Migrations run automatically at startup. On a fresh database the server seeds
it from the markdown files in `public/content/`; later boots leave existing
rows alone.

Server listens on `http://localhost:3000`. The content watcher reindexes the
markdown files for `/sitemap.xml`; add, edit, or delete a post and the index
updates immediately. A 5-hour poll runs alongside as a fallback.

## Environment

| Variable | Required | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | yes | PostgreSQL connection string. |
| `AUTH_TOKEN` | yes | Bearer token for `POST /posts` and `GET /migrate`. |
| `SITE_URL` | no | Absolute origin (no trailing slash) for canonical/OG URLs, `/robots.txt`, `/sitemap.xml`. Defaults to `http://localhost:3000`. |

Generate a token with `openssl rand -hex 32`.

## Test

Tests use `#[sqlx::test]`, which creates throwaway databases on the server in
`DATABASE_URL` and applies `migrations/` first:

```sh
DATABASE_URL=postgres://tinythought:tinythought@localhost:5432/tinythought cargo test
```

The `sqlx::query!` macros are checked against a live database during the build.
For DB-free builds, regenerate the committed `.sqlx` cache after changing any
query or migration:

```sh
DATABASE_URL=... sqlx migrate run --source migrations
DATABASE_URL=... cargo sqlx prepare
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

The first boot on an empty database seeds it from these files. Editing a file
after seeding updates the sitemap but not the database copy.

## Routes

- `/` — all categories + all published posts
- `/health` — liveness probe (no database access)
- `/<category-slug>` — posts in one category
- `/posts/<article-slug>` — a single post
- `/static/*` — CSS, served from `public/static`
- `/sitemap.xml` — every category + published post, for search engines
- `/robots.txt` — points crawlers at the sitemap
- `POST /posts` — create a post (Bearer `AUTH_TOKEN`)
- `GET /migrate` — copy the file index into the database (Bearer `AUTH_TOKEN`)

## Deploy to the VPS

The container joins the shared `vps-edge` network as `tinythought:3000` (Caddy
reaches it) and the `vps-data` network (PostgreSQL at `db:5432`). This
repository owns the app; `vps-infra-biznetgio` owns Caddy, PostgreSQL, and the
Docker networks.

### One-time server preparation

1. Create the app directory:

   ```sh
   sudo install -d -m 750 -o DEPLOY_USER -g DEPLOY_GROUP /opt/tinythought
   ```

2. Create the database and role by running
   `ops/provision-tinythought.sql` from the infra repository on the VPS (set
   the password there, then run the command in that file's header).

3. Create `/opt/tinythought/.env`, readable only by the deployment user:

   ```sh
   DATABASE_URL=postgres://tinythought:<password>@db:5432/tinythought
   AUTH_TOKEN=<openssl rand -hex 32>
   SITE_URL=https://blog.suprie.id
   ```

   `chmod 600 /opt/tinythought/.env`

4. Make the GHCR package public (or run `docker login ghcr.io` once as the
   deployment user) so the server can pull the image.

5. Add a DNS A record for `blog.suprie.id` pointing at the VPS.

### GitHub configuration

Create a `production` environment (or use repository settings) with the same
secrets as the infra repository:

| Secret | Value |
| --- | --- |
| `VPS_HOST` | VPS IP address or SSH hostname |
| `VPS_USER` | SSH deployment user |
| `VPS_SSH_KEY` | Dedicated private SSH key, without a passphrase |
| `VPS_KNOWN_HOSTS` | Verified SSH known-hosts entry |
| `VPS_PORT` *(variable)* | Custom SSH port; defaults to `22` |

### Deploy

Push to `main`, or run **Actions → Test, build, deploy → Run workflow**. The
workflow:

1. runs `cargo fmt --check` and the test suite against PostgreSQL;
2. builds the image and pushes `latest` + the commit SHA to
   `ghcr.io/suprie/tinythought`;
3. copies `compose.yml` and `scripts/` to `/opt/tinythought`, then pulls and
   recreates the app pinned to that commit SHA.

Deploy the Caddy route from the infra repository around the same time — the
`Caddyfile.d/10-tinythought.caddy` file there proxies `blog.suprie.id` to
`tinythought:3000`. Until the app is running, Caddy returns 502.

Verify after the first deploy:

```sh
curl -fsS https://blog.suprie.id/health   # ok
curl -fsS https://blog.suprie.id/ | head  # seeded posts
```

## Layout

```
src/
  content.rs                 indexing: scan, parse frontmatter/markdown, build Index
  views.rs                   Askama template structs
  routes.rs                  Axum handlers
  lib.rs                     state, router, reindex loop
  main.rs                    entrypoint, migrations, first-boot seed
  repositories/              PostgreSQL access (categories, posts)
  services/                  use cases
templates/                   Askama .html templates
public/                      static assets + content/
migrations/                  sqlx migrations (PostgreSQL)
.sqlx/                       offline query metadata for DB-free builds
Dockerfile                   multi-stage release build
compose.yml                  VPS deployment (vps-edge + vps-data)
scripts/deploy-vps.sh        pull + recreate on the server
.github/workflows/deploy.yml test, build, push, deploy
tests/                       integration tests (currently commented out)
```
