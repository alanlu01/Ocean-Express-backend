use axum::{Router, routing::post, extract::State, Json};
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;
use axum::http::StatusCode;
use crate::routes::common::{ApiResult, data_response, data_response_with_status, error_response, document_id};

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

    let user_doc = doc! {
        "name": &payload.name,
        "email": &payload.email,
        "password": &payload.password,
        "phone": &payload.phone,
        "role": "customer"
    };

    let insert = collection.insert_one(user_doc)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let id = insert.inserted_id.as_object_id().map(|oid| oid.to_hex());

    let data = doc! {
        "id": id.unwrap_or_default(),
        "email": payload.email,
        "role": "customer",
        "phone": payload.phone
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

    let password = user_doc.get_str("password").unwrap_or("");
    if password != payload.password {
        return Err(error_response(StatusCode::UNAUTHORIZED, "auth.invalid", "invalid credentials"));
    }

    let token = mongodb::bson::oid::ObjectId::new().to_hex();
    let user_id = document_id(&user_doc).unwrap_or_default();
    let user_data = doc! {
        "id": user_id,
        "email": payload.email,
        "role": user_doc.get_str("role").unwrap_or("customer"),
        "phone": user_doc.get_str("phone").unwrap_or("")
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
