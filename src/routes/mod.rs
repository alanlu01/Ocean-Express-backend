use axum::Router;
use mongodb::Database;

// api routes all import here
mod retaurants;
mod menu;

pub fn api_router(db: Database) -> Router{
    // merge all routes(an api is an endpoint) here
    Router::new()
    .nest("/restaurants", retaurants::home_page_router(db.clone()))
    .nest("/shops", menu::menu_router(db))
}
