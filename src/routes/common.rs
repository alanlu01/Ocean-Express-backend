use axum::{Json, http::{StatusCode, HeaderMap}, response::{IntoResponse, Response}};
use chrono::{NaiveDate, NaiveDateTime};
use mongodb::bson::{doc, Bson, Document, DateTime};
use std::time::{SystemTime, UNIX_EPOCH};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, Algorithm};
use serde::{Deserialize, Serialize};

pub type ApiResult = Result<Response, (StatusCode, Json<Document>)>;

pub fn data_response(value: Bson) -> Response{
    Json(doc! { "data": value }).into_response()
}

pub fn data_response_with_status(status: StatusCode, value: Bson) -> Response{
    (status, Json(doc! { "data": value })).into_response()
}

pub fn error_response(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Document>){
    (status, Json(doc! { "message": message, "code": code }))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub role: String,
    pub exp: usize,
    #[serde(rename = "restaurantId", skip_serializing_if = "Option::is_none")]
    pub restaurant_id: Option<String>,
}

pub fn get_string(doc: &Document, key: &str) -> Option<String>{
    doc.get(key).and_then(Bson::as_str).map(|s| s.to_string())
}

pub fn get_bool(doc: &Document, key: &str) -> Option<bool>{
    doc.get(key).and_then(Bson::as_bool)
}

pub fn get_f64(doc: &Document, key: &str) -> Option<f64>{
    match doc.get(key) {
        Some(Bson::Double(v)) => Some(*v),
        Some(Bson::Int32(v)) => Some(*v as f64),
        Some(Bson::Int64(v)) => Some(*v as f64),
        Some(Bson::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

pub fn get_i64(doc: &Document, key: &str) -> Option<i64>{
    doc.get(key).and_then(Bson::as_i64)
}

pub fn get_array(doc: &Document, key: &str) -> Option<Vec<Bson>>{
    doc.get_array(key).ok().map(|arr| arr.clone())
}

pub fn document_id(doc: &Document) -> Option<String>{
    if let Some(id) = get_string(doc, "id") {
        return Some(id);
    }
    match doc.get_object_id("_id") {
        Ok(oid) => Some(oid.to_hex()),
        Err(_) => None,
    }
}

pub fn now_millis() -> i64{
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn now_datetime() -> DateTime{
    DateTime::from_millis(now_millis())
}

fn jwt_secret() -> String{
    std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret-key-change-me".to_string())
}

pub fn sign_token(user_id: &str, email: &str, role: &str, restaurant_id: Option<&str>, ttl_hours: u64) -> Result<String, jsonwebtoken::errors::Error>{
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as usize;
    let exp = now + (ttl_hours as usize * 60 * 60);
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        role: role.to_string(),
        exp,
        restaurant_id: restaurant_id.map(|r| r.to_string()),
    };
    jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(jwt_secret().as_bytes()))
}

pub fn decode_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error>{
    let validation = Validation::new(Algorithm::HS256);
    jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_secret(jwt_secret().as_bytes()), &validation)
        .map(|data| data.claims)
}

pub fn iso_from_bson(value: &Bson) -> Option<String>{
    match value {
        Bson::DateTime(dt) => Some(dt.to_string()),
        Bson::String(s) => Some(s.clone()),
        _ => None,
    }
}

pub fn bearer_token(headers: &HeaderMap) -> Option<String>{
    let header = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    header.strip_prefix("Bearer ").map(|s| s.to_string())
}

pub fn auth_claims(headers: &HeaderMap) -> Result<Claims, (StatusCode, Json<Document>)>{
    let token = bearer_token(headers).ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "auth.invalid", "missing bearer token"))?;
    decode_token(&token).map_err(|_| error_response(StatusCode::UNAUTHORIZED, "auth.invalid", "invalid credentials"))
}

pub fn require_role(headers: &HeaderMap, allowed_roles: &[&str]) -> Result<Claims, (StatusCode, Json<Document>)>{
    let claims = auth_claims(headers)?;
    if allowed_roles.iter().any(|r| r.eq_ignore_ascii_case(&claims.role)) {
        Ok(claims)
    } else {
        Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"))
    }
}

pub fn date_to_millis(date_str: &str) -> Option<i64>{
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let dt = NaiveDateTime::new(date, chrono::NaiveTime::from_hms_opt(0, 0, 0)?);
    Some(dt.and_utc().timestamp_millis())
}

pub fn date_range_to_bson(from: Option<&str>, to: Option<&str>) -> Option<(DateTime, DateTime)>{
    let start = from.and_then(date_to_millis)?;
    let end = to.and_then(date_to_millis)?;
    let end = end + 24 * 60 * 60 * 1000 - 1;
    Some((DateTime::from_millis(start), DateTime::from_millis(end)))
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64{
    let r = 6371.0_f64; // Earth radius in km
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}
