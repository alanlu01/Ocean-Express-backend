use axum::{Router, routing::post, extract::{State}, Json, http::HeaderMap};
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;
use axum::http::StatusCode;

use crate::routes::common::{ApiResult, data_response_with_status, error_response, now_datetime, require_role};

#[derive(Deserialize)]
struct RegisterPushRequest {
    token: String,
    platform: String,
    #[serde(rename = "userId")]
    user_id: Option<String>,
    role: Option<String>,
    #[serde(rename = "restaurantId")]
    restaurant_id: Option<String>,
}

async fn register_push(State(db): State<Database>, headers: HeaderMap, Json(payload): Json<RegisterPushRequest>) -> ApiResult{
    // Any authenticated role may register push tokens (customer/restaurant/deliverer)
    let claims = require_role(&headers, &["customer", "restaurant", "deliverer"])?;

    if payload.token.trim().is_empty() {
        return Err(error_response(StatusCode::BAD_REQUEST, "validation.failed", "token required"));
    }

    let collection = db.collection::<Document>("push_tokens");
    let filter = doc! { "token": &payload.token };
    let update = doc! {
        "$set": {
            "token": &payload.token,
            "platform": &payload.platform,
            "userId": payload.user_id.clone().unwrap_or_else(|| claims.sub.clone()),
            "role": payload.role.clone().unwrap_or_else(|| claims.role.clone()),
            "restaurantId": payload.restaurant_id.clone().unwrap_or_default(),
            "updatedAt": now_datetime(),
        },
        "$setOnInsert": {
            "createdAt": now_datetime(),
        }
    };

    collection.update_one(filter, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    Ok(data_response_with_status(StatusCode::CREATED, Bson::Document(doc! { "ok": true })))
}

pub fn push_router(db: Database) -> Router{
    Router::new()
        .route("/register", post(register_push))
        .with_state(db)
}
