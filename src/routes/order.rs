use axum::{extract::{Path, Query, State}, http::{HeaderMap, StatusCode}, routing::{get, patch, post}, Json, Router};
use futures::stream::TryStreamExt;
use mongodb::{bson::{doc, Bson, Document, oid::ObjectId}, Database};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
	sub: String,
	email: String,
	role: String,
	exp: usize,
}

fn now_unix() -> i64{
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn now_rfc3339() -> String{
    format!("{}", now_unix())
}

fn auth_from_headers(headers: &HeaderMap) -> Result<Claims, (StatusCode, Json<serde_json::Value>)>{
    let auth = headers.get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !auth.to_lowercase().starts_with("bearer ") {
		return Err((StatusCode::UNAUTHORIZED, Json(json!({"message":"missing bearer token"}))));
	}
    let token = auth[7..].trim();
    let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret-key-change-me".to_string());
    let validation = Validation::new(Algorithm::HS256);
    match decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
		Ok(data) => Ok(data.claims),
		Err(_) => Err((StatusCode::UNAUTHORIZED, Json(json!({"message":"invalid token"}))))
	}
}

#[derive(Debug, Deserialize)]
struct OrderItemInput {
	name: String,
	size: String,
	spiciness: String,
	#[serde(rename="addDrink")]
	add_drink: bool,
	quantity: i64,
}

#[derive(Debug, Deserialize)]
struct DeliveryLocationInput { name: String }

#[derive(Debug, Deserialize)]
struct CreateOrderRequest {
	#[serde(rename="restaurantId")]
	restaurant_id: String,
	items: Vec<OrderItemInput>,
	#[serde(rename="deliveryLocation")]
	delivery_location: DeliveryLocationInput,
	notes: Option<String>,
	#[serde(rename="requestedTime")]
	requested_time: Option<String>,
}

// POST /orders
async fn create_order(
	headers: HeaderMap,
	State(db): State<Database>,
	Json(body): Json<CreateOrderRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>{
    let claims = auth_from_headers(&headers)?;
    if claims.role.to_lowercase() != "customer" {
		return Err((StatusCode::FORBIDDEN, Json(json!({"message":"forbidden"}))));
	}

    let orders = db.collection::<Document>("orders");
    let placed_at = now_rfc3339();
    let eta_minutes: i64 = 20;

    let items_bson: Vec<Bson> = body.items.iter().map(|it| {
		Bson::Document(doc!{
			"name": &it.name,
			"size": &it.size,
			"spiciness": &it.spiciness,
			"addDrink": it.add_drink,
			"quantity": it.quantity,
		})
	}).collect();

    // generate a human-friendly order id (prefix ord- + first 6 chars of ObjectId)
    let raw_oid = ObjectId::new();
    let order_id = format!("ord-{}", &raw_oid.to_hex()[..6]);

    let order_doc = doc!{
		"id": &order_id,
		"user_id": &claims.sub,
		"restaurant_id": &body.restaurant_id,
		"items": Bson::Array(items_bson),
		"deliveryLocation": { "name": &body.delivery_location.name },
		"notes": body.notes.clone().unwrap_or_default(),
		"requestedTime": body.requested_time.clone().unwrap_or_default(),
		"status": "available",
		"etaMinutes": eta_minutes,
		"placedAt": &placed_at,
		"statusTimestamps": { "available": now_unix() },
	};

    orders.insert_one(order_doc.clone()).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))?;

    Ok(Json(json!({"data": {"id": order_id, "status": "available", "etaMinutes": eta_minutes } })))
}

#[derive(Debug, Deserialize)]
struct ListQuery { status: Option<String> }

// GET /orders?status=active|history
async fn list_orders(
	headers: HeaderMap,
	State(db): State<Database>,
	Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>{
    let claims = auth_from_headers(&headers)?;
    if claims.role.to_lowercase() != "customer" {
		return Err((StatusCode::FORBIDDEN, Json(json!({"message":"forbidden"}))));
	}

    let (active, history) = (
		vec!["available", "preparing", "delivering"],
		vec!["completed", "cancelled"],
	);

    let statuses = match q.status.as_deref() {
		Some("active") => active,
		Some("history") => history,
		_ => active, // default
	};

    let orders = db.collection::<Document>("orders");
    let filter = doc!{
		"user_id": &claims.sub,
		"status": { "$in": statuses }
	};
    let mut cursor = orders.find(filter).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))?;

    let shops = db.collection::<Document>("shops");
    let mut out_items: Vec<Bson> = Vec::new();

    while let Some(doc) = cursor.try_next().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))? {
		let id = doc.get("id").and_then(Bson::as_str).map(|s| s.to_string()).or_else(|| doc.get("_id").and_then(|b| match b { Bson::ObjectId(oid) => Some(oid.to_hex()), _ => None }));
		let rest_id = doc.get("restaurant_id").and_then(Bson::as_str).unwrap_or("");
		let status = doc.get("status").and_then(Bson::as_str).unwrap_or("");
		let eta = doc.get("etaMinutes").and_then(Bson::as_i64).unwrap_or(0);
		let placed_at = doc.get("placedAt").and_then(Bson::as_str).unwrap_or("");

		// lookup restaurant name
		let mut rest_name = String::new();
		if !rest_id.is_empty() {
			let f = doc!{"$or": [ {"id": rest_id}, {"shop_id": rest_id} ]};
			if let Ok(Some(sdoc)) = shops.find_one(f).await {
				rest_name = sdoc.get("name").and_then(Bson::as_str).map(|s| s.to_string())
					.or_else(|| sdoc.get("shop_name").and_then(Bson::as_str).map(|s| s.to_string()))
					.unwrap_or_default();
			}
		}

		let mut item = Document::new();
		item.insert("id", id.map(Bson::String).unwrap_or(Bson::Null));
		item.insert("restaurantName", Bson::String(rest_name));
		item.insert("status", Bson::String(status.to_string()));
		item.insert("etaMinutes", Bson::Int64(eta));
		item.insert("placedAt", Bson::String(placed_at.to_string()));
		out_items.push(Bson::Document(item));
	}

    Ok(Json(json!({"data": out_items })))
}

// GET /orders/:id
async fn get_order(
	headers: HeaderMap,
	Path(id): Path<String>,
	State(db): State<Database>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>{
    let claims = auth_from_headers(&headers)?;
    if claims.role.to_lowercase() != "customer" {
		return Err((StatusCode::FORBIDDEN, Json(json!({"message":"forbidden"}))));
	}
    let orders = db.collection::<Document>("orders");
    let filter = doc!{ "$and": [ {"user_id": &claims.sub}, { "$or": [{"id": &id}, {"_id": &id}] } ] };
    let found = orders.find_one(filter).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))?;
    match found {
		Some(doc) => Ok(Json(json!({"data": doc }))),
		None => Err((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))
	}
}

// PATCH /orders/:id/cancel
async fn cancel_order(
	headers: HeaderMap,
	Path(id): Path<String>,
	State(db): State<Database>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>{
    let claims = auth_from_headers(&headers)?;
    if claims.role.to_lowercase() != "customer" {
		return Err((StatusCode::FORBIDDEN, Json(json!({"message":"forbidden"}))));
	}
    let orders = db.collection::<Document>("orders");
    let filter = doc!{ "$and": [ {"user_id": &claims.sub}, { "$or": [{"id": &id}, {"_id": &id}] } ] };

    let mut doc_to_update = match orders.find_one(filter.clone()).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))? {
		Some(d) => d,
		None => return Err((StatusCode::NOT_FOUND, Json(json!({"message":"not found"}))))
	};

    doc_to_update.insert("status", Bson::String("cancelled".to_string()));
    let mut ts = doc_to_update.get("statusTimestamps").and_then(Bson::as_document).cloned().unwrap_or_default();
    ts.insert("cancelled", Bson::Int64(now_unix()));
    doc_to_update.insert("statusTimestamps", Bson::Document(ts));

    orders.replace_one(filter, doc_to_update).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"message": e.to_string()}))))?;

    Ok(Json(json!({"data": {"status": "cancelled"}})))
}

pub fn order_router(db: Database) -> Router{
    Router::new()
		.route("/", get(list_orders).post(create_order))
		.route("/{id}", get(get_order))
		.route("/{id}/cancel", patch(cancel_order))
		.with_state(db)
}
