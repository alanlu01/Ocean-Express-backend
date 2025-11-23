use axum::{Router, extract::State, routing::get};
use mongodb::Database;

async fn get_cart(State(db): State<Database>) -> &'static str{
    "cart endpoint"
}

pub fn cart_router(db: Database) -> Router{
    Router::new()
        .route("/", get(get_cart))
        .with_state(db)
}
