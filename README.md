# tinythought

Tiny blog. Rust + Axum + Askama + PostgreSQL. Posts are markdown files with
YAML frontmatter; the database stores the published copy that the site serves.

## Run locally

Start the local PostgreSQL container, then run the app on your machine:

```sh
docker compose -f compose.dev.yml up -d --wait
cp -n .env.example .env
cargo run --bin hash_admin_password
# Set ADMIN_PASSWORD_HASH in .env to the printed hash, inside single quotes.
cargo run
```

The database data persists in a Docker volume. Stop it with
`docker compose -f compose.dev.yml down` when finished. The VPS uses the
separate `compose.yml`.

`cp -n` keeps an existing `.env`. If you already have one, set its
`DATABASE_URL` to `postgres://tinythought:tinythought@localhost:5432/tinythought`
to match `compose.dev.yml`.

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
| `ADMIN_USERNAME` | yes | Username for the single admin account. |
| `ADMIN_PASSWORD_HASH` | yes | Argon2id hash from `cargo run --bin hash_admin_password`. |
| `SITE_URL` | no | Absolute origin (no trailing slash) for canonical/OG URLs, `/robots.txt`, `/sitemap.xml`. Defaults to `http://localhost:3000`. |
| `UPLOAD_DIR` | no | Directory for uploaded post images. Defaults to `uploads`; the container uses `/app/uploads`. |

The password hash command prompts without echoing the password. Set the hash as
`ADMIN_PASSWORD_HASH='$argon2id$...'` so Docker Compose keeps its dollar signs.
Login sessions last 24 hours and are cleared when the app restarts. If you already have a
`.env`, remove `AUTH_TOKEN` and add the two admin variables above.

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
- `/admin` — admin dashboard (username and password login, publish, delete; `noindex`)
- `/uploads/*` — uploaded post images
- `/static/*` — CSS, served from `public/static`
- `/sitemap.xml` — every category + published post, for search engines
- `/robots.txt` — points crawlers at the sitemap

## Deploy to the VPS

The container joins the shared `vps-edge` network as `tinythought:3000` (Caddy
reaches it) and the `vps-data` network (PostgreSQL at `db:5432`). This
repository owns the app; `vps-infra-biznetgio` owns Caddy, PostgreSQL, and the
Docker networks.

### One-time server preparation

1. Create the app directory:

   ```sh
   sudo install -d -m 750 -o DEPLOY_USER -g DEPLOY_GROUP /opt/tinythought
   sudo install -d -m 750 -o 10001 -g 10001 /opt/tinythought/uploads
   ```

2. Create `/opt/tinythought/.env`, readable only by the deployment user:

   ```sh
   TINYTHOUGHT_DB_PASSWORD=<url-safe-random-password>
   DATABASE_URL=postgres://tinythought:${TINYTHOUGHT_DB_PASSWORD}@db:5432/tinythought
   ADMIN_USERNAME=<your username>
   ADMIN_PASSWORD_HASH='<output from cargo run --bin hash_admin_password>'
   SITE_URL=https://blog.suprie.id
   ```

   `chmod 600 /opt/tinythought/.env`

3. For a new database, run the provisioning script on the VPS. It reads
   `TINYTHOUGHT_DB_PASSWORD` from the app's `.env`. Use letters, digits, hyphens,
   and underscores for the password and enter it without quotes.

   ```sh
   bash /opt/vps-infra-biznetgio/ops/provision-tinythought.sh
   ```

   An existing Tinythought installation can keep its current `DATABASE_URL`
   and skip provisioning.

The New post form accepts one optional JPEG, PNG, GIF, or WebP image up to 5 MB.
It applies the image's orientation, shrinks the longest edge to at most 1,600
pixels, and adds a small `tinythought.` watermark in the lower-right corner
before re-encoding without embedded metadata. Images too small for legible
lettering skip the watermark. The app appends the public copy after the
Markdown body and does not store the original. Animated
uploads are saved as animated PNGs. Large images and long animations can exceed
the processing limits even when the source file fits the upload limit.

Uploads persist in `/opt/tinythought/uploads` across container releases. Include
that directory in host backups. Existing uploads keep their original bytes.
Deleting a post leaves its uploaded image in place.

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

### CDN for post images

To add a CDN later, put the blog hostname behind the CDN with Caddy as the
origin. Cache successful `GET` and `HEAD` requests under `/uploads/*` using the
origin's one-hour cache lifetime, and bypass the cache for `/admin` and
`/admin/*`. The post Markdown uses relative `/uploads/…` URLs, so existing and
new posts use the CDN without editing posts or changing the upload workflow.
If you remove an image from storage, purge its URL from the CDN as well.

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
compose.dev.yml              local PostgreSQL for development
compose.yml                  VPS deployment (vps-edge + vps-data)
scripts/deploy-vps.sh        pull + recreate on the server
.github/workflows/deploy.yml test, build, push, deploy
tests/                       integration tests (currently commented out)
```
