use axum::{Router, routing::{get, post, patch}, extract::{State, Path, Query}, Json, http::HeaderMap, response::sse::{Sse, Event}};
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;
use futures::stream::TryStreamExt;
use futures::stream;
use axum::http::StatusCode;
use std::convert::Infallible;
use crate::routes::common::{ApiResult, data_response, data_response_with_status, error_response, document_id, get_string, get_i64, now_datetime, iso_from_bson, bearer_token};

#[derive(Deserialize)]
struct DeliveryLocation {
    name: String,
    lat: Option<f64>,
    lng: Option<f64>,
}

#[derive(Deserialize)]
struct OrderItemRequest {
    menuItemId: String,
    quantity: Option<i64>,
    size: Option<String>,
    spiciness: Option<String>,
    addDrink: Option<bool>,
}

#[derive(Deserialize)]
struct CreateOrderRequest {
    restaurantId: Option<String>,
    deliveryLocation: DeliveryLocation,
    items: Vec<OrderItemRequest>,
    deliveryFee: i64,
    totalAmount: i64,
    notes: Option<String>,
    requestedTime: Option<String>,
}

#[derive(Deserialize)]
struct RatingRequest {
    score: i64,
    comment: Option<String>,
}

#[derive(Deserialize)]
struct OrderListQuery {
    status: Option<String>,
}

async fn find_menu_item(db: &Database, menu_item_id: &str) -> Result<Option<Document>, (StatusCode, Json<Document>)>{
    let collection = db.collection::<Document>("menu");
    let filter = doc! {
        "$or": [
            { "id": menu_item_id },
            { "_id": mongodb::bson::oid::ObjectId::parse_str(menu_item_id).ok() }
        ]
    };
    collection.find_one(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))
}

async fn create_order(State(db): State<Database>, headers: HeaderMap, Json(payload): Json<CreateOrderRequest>) -> ApiResult{
    if payload.items.is_empty() {
        return Err(error_response(StatusCode::BAD_REQUEST, "validation.failed", "Order items required"));
    }

    let mut items: Vec<Bson> = Vec::new();
    let mut restaurant_id: Option<String> = payload.restaurantId.clone();
    let mut restaurant_name: Option<String> = None;

    for item in &payload.items {
        let menu_doc = find_menu_item(&db, &item.menuItemId).await?;
        let Some(menu_doc) = menu_doc else {
            return Err(error_response(StatusCode::BAD_REQUEST, "menu.unavailable", "menu item unavailable"));
        };

        let is_available = menu_doc.get_bool("isAvailable").unwrap_or(true);
        if !is_available {
            return Err(error_response(StatusCode::BAD_REQUEST, "menu.unavailable", "menu item unavailable"));
        }

        if restaurant_id.is_none() {
            restaurant_id = get_string(&menu_doc, "shop_id")
                .or_else(|| get_string(&menu_doc, "restaurantId"))
                .or_else(|| get_string(&menu_doc, "restaurant_id"));
        }

        let price = get_i64(&menu_doc, "price").unwrap_or(0);
        let quantity = item.quantity.unwrap_or(1);
        let mut item_doc = Document::new();
        item_doc.insert("menuItemId", &item.menuItemId);
        item_doc.insert("name", get_string(&menu_doc, "name").unwrap_or_default());
        item_doc.insert("size", item.size.clone().unwrap_or_default());
        item_doc.insert("spiciness", item.spiciness.clone().unwrap_or_default());
        item_doc.insert("addDrink", item.addDrink.unwrap_or(false));
        item_doc.insert("quantity", quantity);
        item_doc.insert("price", price);
        items.push(Bson::Document(item_doc));
    }

    if let Some(rest_id) = restaurant_id.as_deref() {
        let collection = db.collection::<Document>("shops");
        if let Some(rest) = collection.find_one(doc! { "id": rest_id }).await
            .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
            restaurant_name = get_string(&rest, "name");
        }
    }

    let mut location_doc = doc! { "name": &payload.deliveryLocation.name };
    if let Some(lat) = payload.deliveryLocation.lat {
        location_doc.insert("lat", lat);
    }
    if let Some(lng) = payload.deliveryLocation.lng {
        location_doc.insert("lng", lng);
    }

    let now = now_datetime();
    let status = "available";
    let status_history = vec![Bson::Document(doc! {
        "status": status,
        "timestamp": now
    })];

    let order_id = mongodb::bson::oid::ObjectId::new().to_hex();
    let code = order_id.chars().take(6).collect::<String>().to_uppercase();
    let user_id = bearer_token(&headers).unwrap_or_default();
    let order_doc = doc! {
        "id": &order_id,
        "code": &code,
        "userId": user_id,
        "deliveryLocation": location_doc,
        "items": items,
        "deliveryFee": payload.deliveryFee,
        "totalAmount": payload.totalAmount,
        "status": status,
        "statusHistory": status_history,
        "notes": payload.notes,
        "restaurantId": restaurant_id.unwrap_or_default(),
        "restaurantName": restaurant_name.unwrap_or_default(),
        "requestedTime": payload.requestedTime,
        "placedAt": now,
        "createdAt": now,
        "etaMinutes": 20
    };

    let orders = db.collection::<Document>("orders");
    orders.insert_one(order_doc)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    Ok(data_response_with_status(StatusCode::CREATED, Bson::Document(doc! {
        "id": order_id,
        "status": status,
        "etaMinutes": 20
    })))
}

async fn list_orders(State(db): State<Database>, Query(query): Query<OrderListQuery>, headers: HeaderMap) -> ApiResult{
    let statuses = match query.status.as_deref() {
        Some("history") => vec!["delivered", "cancelled"],
        Some("active") => vec!["available", "assigned", "en_route_to_pickup", "picked_up", "delivering"],
        _ => vec![],
    };

    let mut filter = Document::new();
    if !statuses.is_empty() {
        filter.insert("status", doc! { "$in": statuses });
    }
    if let Some(user_id) = bearer_token(&headers) {
        filter.insert("userId", user_id);
    }

    let collection = db.collection::<Document>("orders");
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let mut orders: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        let mut item = Document::new();
        item.insert("id", document_id(&doc).unwrap_or_default());
        item.insert("restaurantName", get_string(&doc, "restaurantName").unwrap_or_default());
        item.insert("status", get_string(&doc, "status").unwrap_or_default());
        item.insert("etaMinutes", get_i64(&doc, "etaMinutes").unwrap_or(0));
        item.insert("totalAmount", get_i64(&doc, "totalAmount").unwrap_or(0));
        if let Some(placed_at) = doc.get("placedAt").and_then(iso_from_bson) {
            item.insert("placedAt", placed_at);
        }
        orders.push(Bson::Document(item));
    }

    Ok(data_response(Bson::Array(orders)))
}

async fn get_order(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    let collection = db.collection::<Document>("orders");
    let filter = doc! { "id": &id };
    let order = collection.find_one(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let Some(order_doc) = order else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };

    let mut data = Document::new();
    data.insert("id", document_id(&order_doc).unwrap_or_default());
    data.insert("restaurantName", get_string(&order_doc, "restaurantName").unwrap_or_default());
    data.insert("deliveryFee", get_i64(&order_doc, "deliveryFee").unwrap_or(0));
    data.insert("totalAmount", get_i64(&order_doc, "totalAmount").unwrap_or(0));
    data.insert("status", get_string(&order_doc, "status").unwrap_or_default());
    data.insert("etaMinutes", get_i64(&order_doc, "etaMinutes").unwrap_or(0));
    data.insert("riderName", get_string(&order_doc, "riderName").unwrap_or_default());
    data.insert("riderPhone", get_string(&order_doc, "riderPhone").unwrap_or_default());
    if let Some(placed_at) = order_doc.get("placedAt").and_then(iso_from_bson) {
        data.insert("placedAt", placed_at);
    }
    if let Some(requested_time) = order_doc.get("requestedTime").and_then(iso_from_bson) {
        data.insert("requestedTime", requested_time);
    }
    if let Some(location) = order_doc.get("deliveryLocation") {
        data.insert("deliveryLocation", location.clone());
    }
    if let Some(notes) = get_string(&order_doc, "notes") {
        data.insert("notes", notes);
    }
    if let Some(user_id) = bearer_token(&headers) {
        if let Some(order_user_id) = get_string(&order_doc, "userId") {
            if order_user_id != user_id {
                return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
            }
        }
    }

    if let Some(rating) = order_doc.get("rating") {
        data.insert("rating", rating.clone());
    }

    if let Ok(items) = order_doc.get_array("items") {
        let mut out_items: Vec<Bson> = Vec::new();
        for item in items {
            if let Bson::Document(item_doc) = item {
                let mut out = Document::new();
                out.insert("menuItemId", get_string(item_doc, "menuItemId").unwrap_or_default());
                out.insert("name", get_string(item_doc, "name").unwrap_or_default());
                out.insert("size", get_string(item_doc, "size").unwrap_or_default());
                out.insert("spiciness", get_string(item_doc, "spiciness").unwrap_or_default());
                out.insert("addDrink", item_doc.get_bool("addDrink").unwrap_or(false));
                out.insert("quantity", get_i64(item_doc, "quantity").unwrap_or(1));
                out.insert("price", get_i64(item_doc, "price").unwrap_or(0));
                out_items.push(Bson::Document(out));
            }
        }
        data.insert("items", Bson::Array(out_items));
    }

    if let Ok(history) = order_doc.get_array("statusHistory") {
        let mut out_history: Vec<Bson> = Vec::new();
        for entry in history {
            if let Bson::Document(entry_doc) = entry {
                let mut out = Document::new();
                if let Some(status) = get_string(entry_doc, "status") {
                    out.insert("status", status);
                }
                if let Some(timestamp) = entry_doc.get("timestamp").and_then(iso_from_bson) {
                    out.insert("timestamp", timestamp);
                }
                out_history.push(Bson::Document(out));
            }
        }
        data.insert("statusHistory", Bson::Array(out_history));
    }

    Ok(data_response(Bson::Document(data)))
}

async fn add_rating(Path(id): Path<String>, State(db): State<Database>, Json(payload): Json<RatingRequest>) -> ApiResult{
    let collection = db.collection::<Document>("orders");
    let rating_doc = doc! {
        "score": payload.score,
        "comment": payload.comment.clone()
    };

    let update = doc! { "$set": { "rating": rating_doc } };
    let result = collection.update_one(doc! { "id": &id }, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.matched_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    }

    Ok(data_response(Bson::Document(doc! {
        "score": payload.score,
        "comment": payload.comment
    })))
}

async fn cancel_order(Path(id): Path<String>, State(db): State<Database>) -> ApiResult{
    let collection = db.collection::<Document>("orders");
    let now = now_datetime();
    let update = doc! {
        "$set": { "status": "cancelled" },
        "$push": { "statusHistory": { "status": "cancelled", "timestamp": now } }
    };

    let result = collection.update_one(doc! { "id": &id }, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.matched_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    }

    Ok(data_response(Bson::Document(doc! { "status": "cancelled" })))
}

async fn stream_orders(headers: HeaderMap) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>>{
    let _ = bearer_token(&headers);
    let event = Event::default().event("order.updated").data("{}");
    Sse::new(stream::once(async move { Ok(event) }))
}

pub fn orders_router(db: Database) -> Router{
    Router::new()
        .route("/", post(create_order).get(list_orders))
        .route("/stream", get(stream_orders))
        .route("/{id}", get(get_order))
        .route("/{id}/rating", post(add_rating))
        .route("/{id}/cancel", patch(cancel_order))
        .with_state(db)
}
