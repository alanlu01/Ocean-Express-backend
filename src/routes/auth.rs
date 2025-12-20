use axum::{Router, extract::State, routing::post, Json};
use axum::http::StatusCode;
use bcrypt::{hash, verify, DEFAULT_COST};
use mongodb::{bson::{doc, Bson, Document, oid::ObjectId}, Database};
use serde::Deserialize;

use crate::routes::common::{ApiResult, data_response, data_response_with_status, document_id, error_response, sign_token, get_string};

#[derive(Deserialize)]
struct RegisterRequest {
    name: String,
    email: String,
    password: String,
    phone: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn register(State(db): State<Database>, Json(payload): Json<RegisterRequest>) -> ApiResult{
    let collection = db.collection::<Document>("users");

    let existing = collection.find_one(doc! { "email": &payload.email })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if existing.is_some() {
        return Err(error_response(StatusCode::BAD_REQUEST, "auth.email_taken", "email exists"));
    }

    let password_hash = hash(&payload.password, DEFAULT_COST)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let user_id = ObjectId::new().to_hex();
    let user_doc = doc! {
        "id": &user_id,
        "name": &payload.name,
        "email": &payload.email,
        "password": password_hash,
        "phone": &payload.phone,
        "role": "customer"
    };

    let insert = collection.insert_one(user_doc)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let id = insert.inserted_id.as_object_id().map(|oid| oid.to_hex()).unwrap_or(user_id);
    let token = sign_token(&id, &payload.email, "customer", None, 24)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let data = doc! {
        "token": token,
        "user": {
            "id": &id,
            "email": &payload.email,
            "role": "customer",
            "phone": &payload.phone,
            "name": &payload.name,
        },
        "restaurantId": ""
    };

    Ok(data_response_with_status(StatusCode::CREATED, Bson::Document(data)))
}

async fn login(State(db): State<Database>, Json(payload): Json<LoginRequest>) -> ApiResult{
    let collection = db.collection::<Document>("users");
    let user = collection.find_one(doc! { "email": &payload.email })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let Some(user_doc) = user else {
        return Err(error_response(StatusCode::UNAUTHORIZED, "auth.invalid", "invalid credentials"));
    };

    let stored = user_doc.get_str("password").unwrap_or("");

    // accept both hashed (preferred) and legacy plaintext for backward compatibility
    let password_ok = verify(&payload.password, stored).unwrap_or(false) || stored == payload.password;
    if !password_ok {
        return Err(error_response(StatusCode::UNAUTHORIZED, "auth.invalid", "invalid credentials"));
    }

    let user_id = document_id(&user_doc).unwrap_or_default();
    let role = user_doc.get_str("role").unwrap_or("customer");
    let restaurant_id = if role.eq_ignore_ascii_case("restaurant") {
        get_string(&user_doc, "restaurantId")
            .or_else(|| get_string(&user_doc, "shop_id"))
            .or_else(|| Some(user_id.clone()))
    } else {
        None
    };

    let token = sign_token(&user_id, &payload.email, role, restaurant_id.as_deref(), 24)
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let user_data = doc! {
        "id": user_id,
        "email": payload.email,
        "role": role,
        "phone": user_doc.get_str("phone").unwrap_or(""),
        "name": user_doc.get_str("name").unwrap_or(""),
        "restaurantId": restaurant_id.clone().unwrap_or_default()
    };

    let data = doc! { "token": token, "user": Bson::Document(user_data) };
    Ok(data_response(Bson::Document(data)))
}

pub fn auth_router(db: Database) -> Router{
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .with_state(db)
}
