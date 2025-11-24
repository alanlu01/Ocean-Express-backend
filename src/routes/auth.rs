
use axum::{Router, routing::post, extract::State, Json, http::StatusCode, response::IntoResponse};
use mongodb::{bson::{doc, Document, Bson, oid::ObjectId}, Database};
use serde::{Deserialize, Serialize};
use serde_json::json;
use bcrypt::{hash, verify, DEFAULT_COST};
use jsonwebtoken::{EncodingKey, Header};
use std::env;

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    name: String,
    email: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    email: String,
    role: String,
    exp: usize,
}

fn make_jwt(user_id: &str, email: &str, role: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret-key-change-me".to_string());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as usize;
    // expire in 24 hours
    let exp = now + 60 * 60 * 24;
    let claims = Claims { sub: user_id.to_string(), email: email.to_string(), role: role.to_string(), exp };
    jsonwebtoken::encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes()))
}

async fn login_handler(State(db): State<Database>, Json(body): Json<LoginRequest>) -> impl IntoResponse {
    let users = db.collection::<Document>("users");

    let filter = doc! { "email": &body.email };
    let found = users.find_one(filter).await;
    match found {
        Ok(Some(doc)) => {
            // get stored password
            let stored = 
                doc.get("password")
                .and_then(Bson::as_str)
                .map(|s| s.to_string());
            if let Some(stored_hash) = stored {
                if verify(&body.password, &stored_hash).unwrap_or(false) {
                    let user_id = 
                        doc.get("id")
                        .and_then(Bson::as_str).map(|s| s.to_string());

                    let user_id = user_id.unwrap();
                    let role = 
                        doc.get("role")
                        .and_then(Bson::as_str)
                        .map(|s| s.to_string()).unwrap();

                    match make_jwt(&user_id, &body.email, &role) {
                        Ok(token) => {
                            let resp = json!({ "data": { "token": token, "user": { "id": user_id, "email": body.email, "role": role } } });
                            return (StatusCode::OK, Json(resp)).into_response();
                        }
                        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": "token error", "detail": e.to_string() }))).into_response(),
                    }
                }
            }

            (StatusCode::UNAUTHORIZED, Json(json!({ "message": "invalid credentials", "code": "auth.invalid" }))).into_response()
        }
        Ok(None) => (StatusCode::UNAUTHORIZED, Json(json!({ "message": "invalid credentials", "code": "auth.invalid" }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": e.to_string() }))).into_response(),
    }
}

async fn register_handler(State(db): State<Database>, Json(body): Json<RegisterRequest>) -> impl IntoResponse {
    let users = db.collection::<Document>("users");

    // check if email exists
    let filter = doc! { "email": &body.email };
    match users.find_one(filter).await {
        Ok(Some(_)) => return (StatusCode::BAD_REQUEST, Json(json!({ "message": "email exists", "code": "auth.email_taken" }))).into_response(),
        Ok(None) => {}
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": e.to_string() }))).into_response(),
    }

    // hash password
    let hashed = match hash(&body.password, DEFAULT_COST) {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": e.to_string() }))).into_response(),
    };

    // create user id from ObjectId
    let oid = ObjectId::new();
    let user_id = oid.to_hex();

    let new_user = doc! {
        "id": user_id.clone(),
        "name": &body.name,
        "email": &body.email,
        "password": hashed,
        "role": "customer",
    };

    if let Err(e) = users.insert_one(new_user).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": e.to_string() }))).into_response();
    }

    // create token
    match make_jwt(&user_id, &body.email, "customer") {
        Ok(token) => {
            let resp = json!({ "data": { "token": token, "user": { "id": user_id, "email": body.email, "role": "customer" } } });
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "message": "token error", "detail": e.to_string() }))).into_response(),
    }
}

pub fn auth_router(db: Database) -> Router{
    Router::new()
        .route("/login", post(login_handler))
        .route("/register", post(register_handler))
        .with_state(db)
}