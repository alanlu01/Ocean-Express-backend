use axum::Router;
use mongodb::Database;

// api routes all import here
mod auth;
mod menu;
mod order;
mod retaurants;

pub fn api_router(db: Database) -> Router{
    // merge all routes(an api is an endpoint) here
    Router::new()
    .nest("/restaurants", retaurants::home_page_router(db.clone()))
    .nest("/restaurants", menu::menu_router(db.clone()))
    .nest("/orders", order::order_router(db.clone()))
    .nest("/auth", auth::auth_router(db.clone()))
}
