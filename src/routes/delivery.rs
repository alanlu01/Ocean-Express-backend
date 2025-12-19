use axum::{Router, routing::{get, post, patch}, extract::{State, Path, Query}, Json, http::HeaderMap};
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;
use futures::stream::TryStreamExt;
use axum::http::StatusCode;
use crate::routes::common::{ApiResult, data_response, error_response, document_id, get_i64, now_datetime, get_string, bearer_token, date_range_to_bson, iso_from_bson};

#[derive(Deserialize)]
struct AcceptRequest {
    riderName: Option<String>,
    riderPhone: Option<String>,
}

#[derive(Deserialize)]
struct StatusUpdateRequest {
    status: String,
}

#[derive(Deserialize)]
struct IncidentRequest {
    note: String,
}

#[derive(Deserialize)]
struct LocationRequest {
    name: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    category: Option<String>,
}

#[derive(Deserialize)]
struct HistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Deserialize)]
struct EarningsQuery {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct NotificationsQuery {
    sinceId: Option<String>,
    since: Option<String>,
}

fn map_delivery(order: &Document) -> Document{
    let mut delivery = Document::new();
    delivery.insert("id", document_id(order).unwrap_or_default());
    delivery.insert("code", get_string(order, "code").unwrap_or_default());
    delivery.insert("status", get_string(order, "status").unwrap_or_default());
    delivery.insert("delivererId", get_string(order, "delivererId").unwrap_or_default());
    delivery.insert("fee", get_i64(order, "deliveryFee").unwrap_or(0));
    delivery.insert("distanceKm", get_i64(order, "distanceKm").unwrap_or(0));
    delivery.insert("etaMinutes", get_i64(order, "etaMinutes").unwrap_or(0));
    delivery.insert("canPickup", order.get_bool("canPickup").unwrap_or(true));

    if let Some(location) = order.get("deliveryLocation") {
        delivery.insert("dropoff", location.clone());
    }

    let merchant = if let Some(merchant) = order.get("merchant") {
        merchant.clone()
    } else {
        Bson::Document(doc! { "name": get_string(order, "restaurantName").unwrap_or_default() })
    };
    delivery.insert("merchant", merchant);

    if let Some(customer) = order.get("customer") {
        delivery.insert("customer", customer.clone());
    } else {
        delivery.insert("customer", Bson::Document(doc! { "name": Bson::Null, "phone": Bson::Null }));
    }

    delivery
}

async fn list_available(State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    if bearer_token(&headers).is_none() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("orders");
    let filter = doc! { "status": "available" };
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;

    let mut deliveries: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        deliveries.push(Bson::Document(map_delivery(&doc)));
    }

    Ok(data_response(Bson::Array(deliveries)))
}

async fn get_delivery(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    let collection = db.collection::<Document>("orders");
    let order = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(order_doc) = order else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };
    if get_string(&order_doc, "status").as_deref() != Some("available") {
        let deliverer_id = bearer_token(&headers).unwrap_or_default();
        if deliverer_id.is_empty() || get_string(&order_doc, "delivererId").as_deref() != Some(&deliverer_id) {
            return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
        }
    }

    Ok(data_response(Bson::Document(map_delivery(&order_doc))))
}

async fn accept_delivery(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap, Json(payload): Json<AcceptRequest>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("orders");
    let order = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(order_doc) = order else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };
    if get_string(&order_doc, "status").as_deref() != Some("available") {
        return Err(error_response(StatusCode::BAD_REQUEST, "order.conflict", "order not available"));
    }
    let now = now_datetime();
    let update = doc! {
        "$set": {
            "status": "assigned",
            "delivererId": &deliverer_id,
            "riderName": payload.riderName.unwrap_or_default(),
            "riderPhone": payload.riderPhone.unwrap_or_default()
        },
        "$push": { "statusHistory": { "status": "assigned", "timestamp": now } }
    };
    let result = collection.update_one(doc! { "id": &id }, update)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    if result.matched_count == 0 {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    }
    let updated = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"))?;
    Ok(data_response(Bson::Document(map_delivery(&updated))))
}

async fn list_active(State(db): State<Database>, headers: HeaderMap) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("orders");
    let filter = doc! {
        "delivererId": deliverer_id,
        "status": { "$in": ["assigned", "en_route_to_pickup", "picked_up", "delivering"] }
    };
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut deliveries: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        deliveries.push(Bson::Document(map_delivery(&doc)));
    }
    Ok(data_response(Bson::Array(deliveries)))
}

async fn list_history(State(db): State<Database>, headers: HeaderMap, Query(query): Query<HistoryQuery>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("orders");
    let mut filter = doc! {
        "delivererId": deliverer_id,
        "status": { "$in": ["delivered", "cancelled"] }
    };
    if let Some((start, end)) = date_range_to_bson(query.from.as_deref(), query.to.as_deref()) {
        filter.insert("placedAt", doc! { "$gte": start, "$lte": end });
    }
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut deliveries: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        deliveries.push(Bson::Document(map_delivery(&doc)));
    }
    Ok(data_response(Bson::Array(deliveries)))
}

async fn update_status(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap, Json(payload): Json<StatusUpdateRequest>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("orders");
    let order = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let Some(order_doc) = order else {
        return Err(error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"));
    };
    if get_string(&order_doc, "delivererId").as_deref() != Some(&deliverer_id) {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
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
    let updated = collection.find_one(doc! { "id": &id })
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, "order.not_found", "Order not found"))?;
    Ok(data_response(Bson::Document(map_delivery(&updated))))
}

async fn report_incident(Path(id): Path<String>, State(db): State<Database>, headers: HeaderMap, Json(payload): Json<IncidentRequest>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("delivery_incidents");
    let now = now_datetime();
    let incident_doc = doc! {
        "orderId": &id,
        "delivererId": deliverer_id,
        "note": payload.note,
        "createdAt": now
    };
    collection.insert_one(incident_doc)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    Ok(data_response(Bson::Document(doc! { "status": "reported" })))
}

async fn list_locations(State(db): State<Database>) -> ApiResult{
    let collection = db.collection::<Document>("delivery_locations");
    let mut cursor = collection.find(doc! {})
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut grouped: std::collections::BTreeMap<String, Vec<Bson>> = std::collections::BTreeMap::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        let category = get_string(&doc, "category").unwrap_or_else(|| "default".to_string());
        let item = doc! {
            "name": get_string(&doc, "name").unwrap_or_default(),
            "lat": doc.get_f64("lat").ok(),
            "lng": doc.get_f64("lng").ok()
        };
        grouped.entry(category).or_default().push(Bson::Document(item));
    }
    let locations: Vec<Bson> = grouped.into_iter().map(|(category, items)| {
        Bson::Document(doc! { "category": category, "items": items })
    }).collect();
    Ok(data_response(Bson::Array(locations)))
}

async fn update_location(Path(_id): Path<String>, State(db): State<Database>, Json(payload): Json<LocationRequest>) -> ApiResult{
    let collection = db.collection::<Document>("delivery_locations");
    let mut location = Document::new();
    if let Some(name) = payload.name {
        location.insert("name", name);
    }
    if let Some(lat) = payload.lat {
        location.insert("lat", lat);
    }
    if let Some(lng) = payload.lng {
        location.insert("lng", lng);
    }
    if let Some(category) = payload.category {
        location.insert("category", category);
    }
    collection.insert_one(location)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    Ok(data_response(Bson::Document(doc! { "ok": true })))
}

async fn list_earnings(State(db): State<Database>, headers: HeaderMap, Query(query): Query<EarningsQuery>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let Some((start, end)) = date_range_to_bson(Some(&query.from), Some(&query.to)) else {
        return Err(error_response(StatusCode::BAD_REQUEST, "validation.failed", "invalid date range"));
    };
    let collection = db.collection::<Document>("orders");
    let filter = doc! {
        "delivererId": deliverer_id,
        "status": "delivered",
        "placedAt": { "$gte": start, "$lte": end }
    };
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut total_earnings = 0i64;
    let mut total_tasks = 0i64;
    let mut by_day: std::collections::BTreeMap<String, (i64, i64)> = std::collections::BTreeMap::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        total_tasks += 1;
        let fee = get_i64(&doc, "deliveryFee").unwrap_or(0);
        total_earnings += fee;
        let date_key = doc.get("placedAt")
            .and_then(iso_from_bson)
            .and_then(|iso| iso.get(0..10).map(|s| s.to_string()))
            .unwrap_or_else(|| query.from.clone());
        let entry = by_day.entry(date_key).or_insert((0, 0));
        entry.0 += fee;
        entry.1 += 1;
    }
    let by_day_vec: Vec<Bson> = by_day.into_iter().map(|(date, (earnings, count))| {
        Bson::Document(doc! { "date": date, "totalEarnings": earnings, "taskCount": count })
    }).collect();
    Ok(data_response(Bson::Document(doc! {
        "from": query.from,
        "to": query.to,
        "currency": "TWD",
        "totalEarnings": total_earnings,
        "totalTasks": total_tasks,
        "byDay": by_day_vec
    })))
}

async fn list_notifications(State(db): State<Database>, headers: HeaderMap, Query(query): Query<NotificationsQuery>) -> ApiResult{
    let deliverer_id = bearer_token(&headers).unwrap_or_default();
    if deliverer_id.is_empty() {
        return Err(error_response(StatusCode::FORBIDDEN, "auth.forbidden", "forbidden"));
    }
    let collection = db.collection::<Document>("delivery_notifications");
    let mut filter = doc! { "delivererId": deliverer_id };
    if let Some(since_id) = query.sinceId {
        filter.insert("id", doc! { "$gt": since_id });
    }
    if let Some(since) = query.since {
        filter.insert("createdAt", since);
    }
    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    let mut notifications: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))? {
        let mut item = Document::new();
        item.insert("id", get_string(&doc, "id").unwrap_or_default());
        item.insert("type", get_string(&doc, "type").unwrap_or_default());
        item.insert("taskId", get_string(&doc, "taskId").unwrap_or_default());
        item.insert("status", get_string(&doc, "status").unwrap_or_default());
        if let Some(created_at) = doc.get("createdAt").and_then(iso_from_bson) {
            item.insert("createdAt", created_at);
        }
        notifications.push(Bson::Document(item));
    }
    Ok(data_response(Bson::Array(notifications)))
}

pub fn delivery_router(db: Database) -> Router{
    Router::new()
        .route("/available", get(list_available))
        .route("/active", get(list_active))
        .route("/history", get(list_history))
        .route("/earnings", get(list_earnings))
        .route("/notifications", get(list_notifications))
        .route("/locations", get(list_locations))
        .route("/{id}", get(get_delivery).post(accept_delivery))
        .route("/{id}/accept", post(accept_delivery))
        .route("/{id}/status", patch(update_status))
        .route("/{id}/incident", post(report_incident))
        .route("/{id}/location", post(update_location))
        .with_state(db)
}
