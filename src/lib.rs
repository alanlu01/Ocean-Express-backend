#![allow(non_snake_case)]

use axum::Router;
use mongodb::Database;

// import and merge all route here
mod routes;

pub fn app(db: Database) -> Router{
    Router::new()
        .merge(routes::api_router(db))
    //.with_state(db)
}
