use axum::Router;
use mongodb::Database;

// api routes all import here
mod auth;
mod common;
mod delivery;
mod menu;
mod orders;
mod restaurant;
mod retaurants;

pub fn api_router(db: Database) -> Router{
    // merge all routes(an api is an endpoint) here
    Router::new()
    .nest("/auth", auth::auth_router(db.clone()))
    .nest("/restaurants", retaurants::home_page_router(db.clone()))
    .nest("/restaurants", menu::menu_router(db.clone()))
    .nest("/orders", orders::orders_router(db.clone()))
    .nest("/delivery", delivery::delivery_router(db.clone()))
    .nest("/restaurant", restaurant::restaurant_router(db.clone()))
}
