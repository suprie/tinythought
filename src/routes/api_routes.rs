use crate::handlers;
use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::routing::{get, post};

pub fn build_protected_router(state: SharedState) -> Router {
    Router::new()
        .route("/migrate", get(handlers::migrate))
        .route("/posts", post(handlers::posts))
        .route_layer(from_fn_with_state(state.clone(), auth))
        .with_state(state) // Important: attach state to router
}

async fn auth(
    State(state): State<SharedState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let ok = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|f| f.to_str().ok())
        .and_then(|f| f.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token);
    if ok {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
