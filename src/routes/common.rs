use axum::{Json, http::{StatusCode, HeaderMap}, response::{IntoResponse, Response}};
use chrono::{NaiveDate, NaiveDateTime};
use mongodb::bson::{doc, Bson, Document, DateTime};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn get_string(doc: &Document, key: &str) -> Option<String>{
    doc.get(key).and_then(Bson::as_str).map(|s| s.to_string())
}

pub fn get_bool(doc: &Document, key: &str) -> Option<bool>{
    doc.get(key).and_then(Bson::as_bool)
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
