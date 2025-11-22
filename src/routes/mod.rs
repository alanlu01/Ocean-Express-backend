use axum::Router;

// api routes all import here
mod home_page;

pub fn api_router() -> Router{
    // merge all routes(an api is an endpoint) here
    Router::new()
        .nest("/home_page", home_page::home_page_router())
}
