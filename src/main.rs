use std::{env, path::PathBuf, sync::Arc, time::Duration};

use miniblog::{
    auth::AdminAuth,
    build_router, build_state,
    repositories::{PgCategoryRepository, PgPostRepository},
    services::{CategoryService, MigrationService, PostService},
    spawn_content_watcher, spawn_reindex_loop,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let content_dir = PathBuf::from("public/content");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&env::var("DATABASE_URL")?)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let category_repository = Arc::new(PgCategoryRepository::new(pool.clone()));
    let category_service = Arc::new(CategoryService::new(category_repository.clone()));

    let post_repository = Arc::new(PgPostRepository::new(pool));
    let post_service = Arc::new(PostService::new(post_repository.clone()));
    let migration_service = Arc::new(MigrationService::new(category_repository, post_repository));

    let username = env::var("ADMIN_USERNAME")?;
    let password_hash = env::var("ADMIN_PASSWORD_HASH")?;
    let secure_cookie = env::var("SITE_URL")
        .unwrap_or_default()
        .starts_with("https://");
    let admin_auth = AdminAuth::new(username, password_hash, secure_cookie)?;

    let state = build_state(
        content_dir,
        admin_auth,
        category_service,
        post_service,
        migration_service,
    );
    {
        let index = state.index.read().unwrap();
        println!(
            "indexed {} posts across {} categories",
            index.all_posts.len(),
            index.categories.len()
        );
    }

    // First boot on a fresh database: seed it from the markdown files that
    // ship with the image. Later boots leave the database alone.
    if state.post_services.get_posts().await?.is_empty() {
        let (categories, posts) = {
            let index = state.index.read().unwrap();
            (index.categories.clone(), index.all_posts.clone())
        };
        state.migration_services.migrate(&categories, &posts).await;
        println!(
            "seeded {} posts from {}",
            posts.len(),
            state.content_dir.display()
        );
    }

    spawn_reindex_loop(state.clone());
    spawn_content_watcher(state.clone());

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to 0.0.0.0:3000");
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
    return Ok(());
}
