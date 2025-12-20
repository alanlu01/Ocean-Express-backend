#![allow(non_snake_case)]

use axum::{Router, routing::{get, patch}, extract::{State, Path, Query}, Json, http::HeaderMap};
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;
use futures::stream::TryStreamExt;
use axum::http::StatusCode;
use std::collections::HashMap;
use crate::routes::common::{ApiResult, data_response, data_response_with_status, error_response, document_id, get_string, get_i64, get_bool, get_array, now_datetime, iso_from_bson, now_millis, require_role};

#[derive(Deserialize)]
struct OrderListQuery {
    status: Option<String>,
    restaurantId: Option<String>,
}

#[derive(Deserialize)]
struct MenuListQuery {
    restaurantId: Option<String>,
}

#[derive(Deserialize)]
struct MenuItemRequest {
    name: String,
    description: Option<String>,
    price: i64,
    sizes: Option<Vec<String>>,
    spicinessOptions: Option<Vec<String>>,
    imageUrl: Option<String>,
    isAvailable: bool,
    sortOrder: i64,
    allergens: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    restaurantId: Option<String>,
}

#[derive(Deserialize)]
struct MenuItemPatch {
    name: Option<String>,
    description: Option<String>,
    price: Option<i64>,
    sizes: Option<Vec<String>>,
    spicinessOptions: Option<Vec<String>>,
    imageUrl: Option<String>,
    isAvailable: Option<bool>,
    sortOrder: Option<i64>,
    allergens: Option<Vec<String>>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct StatusUpdateRequest {
    status: String,
}

#[derive(Deserialize)]
struct ReportQuery {
    range: Option<String>,
    restaurantId: Option<String>,
}

async fn load_customer(db: &Database, user_id: &str) -> Option<Document>{
    let users = db.collection::<Document>("users");
    match users.find_one(doc! { "id": user_id }).await {
        Ok(Some(user_doc)) => {
            let mut c = Document::new();
            if let Some(name) = get_string(&user_doc, "name") {
                c.insert("name", name);
            }
            if let Some(phone) = get_string(&user_doc, "phone") {
                c.insert("phone", phone);
            }
            if let Some(email) = get_string(&user_doc, "email") {
                c.insert("email", email);
            }
            c.insert("id", user_id.to_string());
            Some(c)
        }
        _ => None,
    }
}

fn can_transition(current: &str, next: &str) -> bool{
    match (current, next) {
        ("available", "assigned") => true,
        ("assigned", "en_route_to_pickup") => true,
        ("en_route_to_pickup", "picked_up") => true,
        ("picked_up", "delivering") => true,
        ("delivering", "delivered") => true,
        (_, "cancelled") if current != "delivered" && current != "cancelled" => true,
        _ => false,
    }
}

fn map_menu_item(doc: &Document) -> Document{
    let mut item = Document::new();
    let id = document_id(doc);
    item.insert("id", id.unwrap_or_default());
    item.insert("name", get_string(doc, "name").unwrap_or_default());
    item.insert("description", get_string(doc, "description").unwrap_or_default());
    let price = match doc.get("price") {
        Some(Bson::Int32(v)) => *v as i64,
        Some(Bson::Int64(v)) => *v,
        Some(Bson::Double(v)) => v.round() as i64,
        Some(Bson::String(s)) => s.parse::<i64>().unwrap_or(0),
        _ => get_i64(doc, "price").unwrap_or(0),
    };
    item.insert("price", price);
    if let Some(sizes) = get_array(doc, "sizes").or_else(|| get_array(doc, "size")) {
        item.insert("sizes", Bson::Array(sizes));
    }
    if let Some(spiciness) = get_array(doc, "spicinessOptions") {
        item.insert("spicinessOptions", Bson::Array(spiciness));
    }
    item.insert("imageUrl", get_string(doc, "imageUrl").unwrap_or_default());
    item.insert("isAvailable", Bson::Boolean(get_bool(doc, "isAvailable").unwrap_or(true)));
    item.insert("sortOrder", Bson::Int64(get_i64(doc, "sortOrder").unwrap_or(0)));
    item.insert("allergens", Bson::Array(get_array(doc, "allergens").unwrap_or_default()));
    item.insert("tags", Bson::Array(get_array(doc, "tags").unwrap_or_default()));
    item
}

async fn list_orders(State(db): State<Database>, Query(query): Query<OrderListQuery>, headers: HeaderMap) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let mut filter = Document::new();
    let restaurant_id = query.restaurantId.unwrap_or_else(|| claims.sub.clone());
    filter.insert("restaurantId", restaurant_id);

    if let Some(status) = query.status {
        let statuses = match status.as_str() {
            "history" => vec!["delivered", "cancelled"],
            "active" => vec!["available", "assigned", "en_route_to_pickup", "picked_up", "delivering"],
            _ => vec![],
        };
        if !statuses.is_empty() {
            filter.insert("status", doc! { "$in": statuses });
        }
    }

    let collection = db.collection::<Document>("orders");
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let mut orders: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        let mut order = Document::new();
        order.insert("id", document_id(&doc).unwrap_or_default());
        order.insert("code", get_string(&doc, "code").unwrap_or_default());
        order.insert("status", get_string(&doc, "status").unwrap_or_default());
        if let Some(placed_at) = doc.get("placedAt").and_then(iso_from_bson) {
            order.insert("placedAt", placed_at);
        }
        order.insert("etaMinutes", get_i64(&doc, "etaMinutes").unwrap_or(0));
        order.insert("totalAmount", get_i64(&doc, "totalAmount").unwrap_or(0));
        order.insert("deliveryFee", get_i64(&doc, "deliveryFee").unwrap_or(0));
        if let Some(location) = doc.get("deliveryLocation") {
            order.insert("deliveryLocation", location.clone());
        }
        if let Some(notes) = get_string(&doc, "notes") {
            order.insert("notes", notes);
        }

        if let Some(customer) = doc.get("customer") {
            order.insert("customer", customer.clone());
        } else if let Some(user_id) = get_string(&doc, "userId") {
            if let Some(c) = load_customer(&db, &user_id).await {
                order.insert("customer", c);
            } else {
                order.insert("customer", doc! { "name": Bson::Null, "phone": Bson::Null });
            }
        } else {
            order.insert("customer", doc! { "name": Bson::Null, "phone": Bson::Null });
        }

        if let Ok(items) = doc.get_array("items") {
            let mut out_items: Vec<Bson> = Vec::new();
            for item in items {
                if let Bson::Document(item_doc) = item {
                    let mut out = Document::new();
                    out.insert("id", get_string(item_doc, "menuItemId").unwrap_or_default());
                    out.insert("name", get_string(item_doc, "name").unwrap_or_default());
                    out.insert("size", get_string(item_doc, "size").unwrap_or_default());
                    out.insert("spiciness", get_string(item_doc, "spiciness").unwrap_or_default());
                    out.insert("quantity", get_i64(item_doc, "quantity").unwrap_or(1));
                    out.insert("price", get_i64(item_doc, "price").unwrap_or(0));
                    out_items.push(Bson::Document(out));
                }
            }
            order.insert("items", Bson::Array(out_items));
        }

        orders.push(Bson::Document(order));
    }

    Ok(data_response(Bson::Array(orders)))
}

async fn get_order(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let collection = db.collection::<Document>("orders");
    let order = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(order_doc) = order else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };
    if let Some(rest_id) = get_string(&order_doc, "restaurantId") {
        if rest_id != claims.sub && Some(rest_id.clone()) != get_string(&order_doc, "shop_id") {
            return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
        }
    }

    let mut data = Document::new();
    data.insert("id", document_id(&order_doc).unwrap_or_default());
    data.insert("code", get_string(&order_doc, "code").unwrap_or_default());
    data.insert("riderName", get_string(&order_doc, "riderName").unwrap_or_default());
    data.insert("riderPhone", get_string(&order_doc, "riderPhone").unwrap_or_default());
    if let Some(placed_at) = order_doc.get("placedAt").and_then(iso_from_bson) {
        data.insert("placedAt", placed_at);
    }
    if let Some(location) = order_doc.get("deliveryLocation") {
        data.insert("deliveryLocation", location.clone());
    }
    if let Some(notes) = get_string(&order_doc, "notes") {
        data.insert("notes", notes);
    }
    if let Some(customer) = order_doc.get("customer") {
        data.insert("customer", customer.clone());
    } else if let Some(uid) = get_string(&order_doc, "userId") {
        if let Some(c) = load_customer(&db, &uid).await {
            data.insert("customer", c);
        } else {
            data.insert("customer", doc! { "name": Bson::Null, "phone": Bson::Null });
        }
    } else {
        data.insert("customer", doc! { "name": Bson::Null, "phone": Bson::Null });
    }
    if let Ok(items) = order_doc.get_array("items") {
        let mut out_items: Vec<Bson> = Vec::new();
        for item in items {
            if let Bson::Document(item_doc) = item {
                let mut out = Document::new();
                out.insert("id", get_string(item_doc, "menuItemId").unwrap_or_default());
                out.insert("name", get_string(item_doc, "name").unwrap_or_default());
                out.insert("size", get_string(item_doc, "size").unwrap_or_default());
                out.insert("spiciness", get_string(item_doc, "spiciness").unwrap_or_default());
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

async fn update_order_status(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap, Json(payload): Json<StatusUpdateRequest>) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let collection = db.collection::<Document>("orders");
    let existing = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(order_doc) = existing else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };
    let rest_id = get_string(&order_doc, "restaurantId").unwrap_or_default();
    if !rest_id.is_empty() && rest_id != claims.sub {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let current = get_string(&order_doc, "status").unwrap_or_default();
    if current == "delivered" || current == "cancelled" {
        return Err(error_response(StatusCode::BAD_REQUEST, "order.conflict", "order finalized"));
    }
    if !can_transition(&current, &payload.status) {
        return Err(error_response(StatusCode::BAD_REQUEST, "order.conflict", "invalid status transition"));
    }
    let now = now_datetime();
    let update = doc! {
        "$set": { "status": &payload.status },
        "$push": { "statusHistory": { "status": &payload.status, "timestamp": now } }
    };

    let result = collection.update_one(doc! { "id": &id }, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.matched_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    }

    Ok(data_response(Bson::Document(doc! { "status": payload.status })))
}

async fn list_menu(State(db): State<Database>, Query(query): Query<MenuListQuery>, headers: HeaderMap) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let collection = db.collection::<Document>("menu");
    let restaurant_id = query.restaurantId.unwrap_or_else(|| claims.sub.clone());
    let filter = doc! { "$or": [ { "shop_id": &restaurant_id }, { "restaurantId": &restaurant_id }, { "restaurant_id": &restaurant_id } ] };

    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut items: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        items.push(Bson::Document(map_menu_item(&doc)));
    }

    Ok(data_response(Bson::Array(items)))
}

async fn create_menu_item(State(db): State<Database>, headers: HeaderMap, Json(payload): Json<MenuItemRequest>) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let restaurant_id = payload.restaurantId.clone().unwrap_or_else(|| claims.sub.clone());
    let collection = db.collection::<Document>("menu");
    let id = mongodb::bson::oid::ObjectId::new().to_hex();

    let menu_doc = doc! {
        "id": &id,
        "name": payload.name.clone(),
        "description": payload.description.clone(),
        "price": payload.price,
        "sizes": payload.sizes.clone(),
        "spicinessOptions": payload.spicinessOptions.clone(),
        "imageUrl": payload.imageUrl.clone(),
        "isAvailable": payload.isAvailable,
        "sortOrder": payload.sortOrder,
        "allergens": payload.allergens.clone(),
        "tags": payload.tags.clone(),
        "restaurantId": restaurant_id
    };

    collection.insert_one(menu_doc.clone())
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    Ok(data_response_with_status(StatusCode::CREATED, Bson::Document(map_menu_item(&menu_doc))))
}

async fn update_menu_item(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap, Json(payload): Json<MenuItemPatch>) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let mut update_doc = Document::new();
    if let Some(name) = payload.name {
        update_doc.insert("name", name);
    }
    if let Some(description) = payload.description {
        update_doc.insert("description", description);
    }
    if let Some(price) = payload.price {
        update_doc.insert("price", price);
    }
    if let Some(sizes) = payload.sizes {
        update_doc.insert("sizes", sizes);
    }
    if let Some(spiciness) = payload.spicinessOptions {
        update_doc.insert("spicinessOptions", spiciness);
    }
    if let Some(image_url) = payload.imageUrl {
        update_doc.insert("imageUrl", image_url);
    }
    if let Some(is_available) = payload.isAvailable {
        update_doc.insert("isAvailable", is_available);
    }
    if let Some(sort_order) = payload.sortOrder {
        update_doc.insert("sortOrder", sort_order);
    }
    if let Some(allergens) = payload.allergens {
        update_doc.insert("allergens", allergens);
    }
    if let Some(tags) = payload.tags {
        update_doc.insert("tags", tags);
    }

    if update_doc.is_empty() {
        return Err(error_response(StatusCode::BAD_REQUEST, "validation.failed", "No fields to update"));
    }

    let collection = db.collection::<Document>("menu");
    let existing = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(menu_doc) = existing else {
        return Err(error_response(StatusCode::NOT_FOUND, "menu.unavailable", "Menu item not found"));
    };
    if let Some(rest_id) = get_string(&menu_doc, "restaurantId").or_else(|| get_string(&menu_doc, "shop_id")) {
        if rest_id != claims.sub {
            return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
        }
    }

    let update = doc! { "$set": update_doc };
    let result = collection.update_one(doc! { "id": &id }, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.matched_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "menu.unavailable", "Menu item not found"));
    }

    let updated = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "menu.unavailable", "Menu item not found"))?;
    Ok(data_response(Bson::Document(map_menu_item(&updated))))
}

async fn delete_menu_item(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let collection = db.collection::<Document>("menu");
    let existing = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(menu_doc) = existing else {
        return Err(error_response(StatusCode::NOT_FOUND, "menu.unavailable", "Menu item not found"));
    };
    if let Some(rest_id) = get_string(&menu_doc, "restaurantId").or_else(|| get_string(&menu_doc, "shop_id")) {
        if rest_id != claims.sub {
            return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
        }
    }
    let result = collection.delete_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.deleted_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "menu.unavailable", "Menu item not found"));
    }
    Ok(data_response(Bson::Document(doc! { "ok": true })))
}

async fn reports(State(db): State<Database>, headers: HeaderMap, Query(query): Query<ReportQuery>) -> ApiResult{
    let claims = require_role(&headers, &["restaurant"])?;
    let range = query.range.unwrap_or_else(|| "30d".to_string());
    let now_millis = now_millis();
    let duration_days = match range.as_str() {
        "today" => 1,
        "7d" => 7,
        "30d" => 30,
        _ => 30,
    };
    let start_millis = now_millis - (duration_days as i64 * 24 * 60 * 60 * 1000);

    let mut filter = Document::new();
    let restaurant_id = query.restaurantId.unwrap_or_else(|| claims.sub.clone());
    filter.insert("restaurantId", restaurant_id);

    let collection = db.collection::<Document>("orders");
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let mut total_revenue = 0i64;
    let mut order_count = 0i64;
    let mut items_map: HashMap<String, (String, i64, i64)> = HashMap::new();

    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        let created_at = match doc.get("createdAt") {
            Some(Bson::DateTime(dt)) => dt.timestamp_millis(),
            Some(Bson::String(_)) => now_millis,
            _ => now_millis,
        };
        if created_at < start_millis {
            continue;
        }
        order_count += 1;
        total_revenue += get_i64(&doc, "totalAmount").unwrap_or(0);

        if let Ok(items) = doc.get_array("items") {
            for item in items {
                if let Bson::Document(item_doc) = item {
                    let id = get_string(item_doc, "menuItemId").unwrap_or_default();
                    let name = get_string(item_doc, "name").unwrap_or_default();
                    let quantity = get_i64(item_doc, "quantity").unwrap_or(1);
                    let price = get_i64(item_doc, "price").unwrap_or(0);
                    let entry = items_map.entry(id.clone()).or_insert((name, 0, 0));
                    entry.1 += quantity;
                    entry.2 += quantity * price;
                }
            }
        }
    }

    let mut top_items: Vec<Bson> = items_map.into_iter()
        .map(|(id, (name, quantity, revenue))| {
            Bson::Document(doc! { "id": id, "name": name, "quantity": quantity, "revenue": revenue })
        })
        .collect();
    top_items.sort_by(|a, b| {
        let a_rev = a.as_document().and_then(|d| d.get_i64("revenue").ok()).unwrap_or(0);
        let b_rev = b.as_document().and_then(|d| d.get_i64("revenue").ok()).unwrap_or(0);
        b_rev.cmp(&a_rev)
    });

    let data = doc! {
        "range": range,
        "totalRevenue": total_revenue,
        "orderCount": order_count,
        "topItems": Bson::Array(top_items)
    };

    Ok(data_response(Bson::Document(data)))
}

pub fn restaurant_router(db: Database) -> Router{
    Router::new()
        .route("/orders", get(list_orders))
        .route("/orders/{id}", get(get_order))
        .route("/orders/{id}/status", patch(update_order_status))
        .route("/menu", get(list_menu).post(create_menu_item))
        .route("/menu/{id}", patch(update_menu_item).delete(delete_menu_item))
        .route("/reports", get(reports))
        .with_state(db)
}
