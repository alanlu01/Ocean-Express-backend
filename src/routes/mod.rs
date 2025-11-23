use axum::Router;
use mongodb::Database;

// api routes all import here
mod cart;
mod home_page;

pub fn api_router(db: Database) -> Router{
    // merge all routes(an api is an endpoint) here
    Router::new()
        .nest("/home_page", home_page::home_page_router(db.clone()))
        .nest("/cart", cart::cart_router(db))
}
