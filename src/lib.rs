use axum::{Router, http::StatusCode, routing::get};

pub fn app() -> Router {
    Router::new()
        .route("/", get(root_handler))
        .route("/getProduct", get(get_product))
}

async fn root_handler() -> &'static str {
    "Hello World!"
}

async fn get_product() -> (StatusCode, &'static str) {
    (StatusCode::OK, "Product added successfully")
}
