use axum::Router;

// import and merge all route here
mod routes;

pub fn app() -> Router{
    Router::new().merge(routes::api_router())
}
