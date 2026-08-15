use std::path::PathBuf;

use miniblog::{build_router, build_state, spawn_content_watcher, spawn_reindex_loop};

#[tokio::main]
async fn main() {
    let content_dir = PathBuf::from("public/content");
    let state = build_state(content_dir);
    {
        let index = state.index.read().unwrap();
        println!(
            "indexed {} posts across {} categories",
            index.all_posts.len(),
            index.categories.len()
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
}
