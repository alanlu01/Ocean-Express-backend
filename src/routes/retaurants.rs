use axum::{Router, routing::get, extract::{State, Path}};
use mongodb::{bson::{doc, Bson, Document}, Database};
use futures::stream::TryStreamExt;
use crate::routes::common::{ApiResult, data_response, error_response, get_string, document_id, iso_from_bson, get_i64};
use axum::http::StatusCode;

async fn rating_from_orders(db: &Database, restaurant_id: &str) -> Option<f64>{
    let orders = db.collection::<Document>("orders");
    let mut cursor = orders.find(doc! { "restaurantId": restaurant_id, "rating.score": { "$exists": true } })
        .await
        .ok()?;
    let mut sum = 0f64;
    let mut count = 0f64;
    while let Some(doc) = cursor.try_next().await.ok()? {
        let score = doc.get("rating").and_then(Bson::as_document).and_then(|d| {
            match d.get("score") {
                Some(Bson::Int32(v)) => Some(*v as f64),
                Some(Bson::Int64(v)) => Some(*v as f64),
                Some(Bson::Double(v)) => Some(*v),
                _ => None,
            }
        });
        if let Some(s) = score {
            sum += s;
            count += 1.0;
        }
    }
    if count > 0.0 { Some(sum / count) } else { None }
}

// GET /restaurants
async fn list_restaurants(State(db): State<Database>) -> ApiResult{
    let collections = db.list_collection_names()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("List collections error: {}", e)))?;

    if collections.is_empty() {
        return Ok(data_response(Bson::Array(Vec::new())));
    }

    let coll_name = if collections.iter().any(|c| c == "shops") {
        "shops"
    } else {
        &collections[2]
    };

    let collection = db.collection::<Document>(coll_name);
    let mut cursor = collection.find(doc! {})
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Find error: {}", e)))?;

    let mut items: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Cursor error: {}", e)))? {
        // mapping attributes
        let id = document_id(&doc);
        let name = get_string(&doc, "name");
        let image = get_string(&doc, "imageUrl");
        let mut rating = doc.get("rating").and_then(Bson::as_f64)
            .or_else(|| get_i64(&doc, "rating").map(|v| v as f64));
        if rating.is_none() {
            rating = rating_from_orders(&db, id.as_deref().unwrap_or("")).await;
        }

        let mut item = Document::new();
        item.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("name", match name { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("imageUrl", match image { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("rating", match rating { Some(v) => Bson::Double(v), None => Bson::Null });

        items.push(Bson::Document(item));
    }

    Ok(data_response(Bson::Array(items)))
}

// GET /restaurants/{id}
async fn get_restaurant_by_id(Path(id): Path<String>, State(db): State<Database>) -> ApiResult{
    let collection = db.collection::<Document>("shops");

    // matching by id
    let filter = doc! { "id": &id };

    let found = collection.find_one(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &e.to_string()))?;
    match found {
        Some(doc) => {
            let id = document_id(&doc);
            let name = get_string(&doc, "name");
            let image = get_string(&doc, "imageUrl");
            let address = get_string(&doc, "address");
            let phone = get_string(&doc, "phone");
            let mut rating = doc.get("rating").and_then(Bson::as_f64)
                .or_else(|| get_i64(&doc, "rating").map(|v| v as f64));
            if rating.is_none() {
                rating = rating_from_orders(&db, id.as_deref().unwrap_or("")).await;
            }

            let mut body = Document::new();
            body.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("name", match name { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("imageUrl", match image { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("address", match address { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("phone", match phone { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("rating", match rating { Some(v) => Bson::Double(v), None => Bson::Null });

            Ok(data_response(Bson::Document(body)))
        }
        None => {
            Ok(data_response(Bson::Null))
        }
    }
}

// GET /restaurants/{id}/reviews
async fn list_reviews(Path(id): Path<String>, State(db): State<Database>) -> ApiResult{
    let collection = db.collection::<Document>("reviews");
    let filter = doc! {
        "$or": [
            { "restaurantId": &id },
            { "shop_id": &id },
            { "restaurant_id": &id }
        ]
    };

    let mut items: Vec<Bson> = Vec::new();

    // legacy reviews collection
    let mut cursor = collection.find(filter.clone())
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Find error: {}", e)))?;
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Cursor error: {}", e)))? {
        let mut item = Document::new();
        let id = document_id(&doc);
        let user_name = get_string(&doc, "userName").or_else(|| get_string(&doc, "user_name"));
        let rating = doc.get("rating").and_then(Bson::as_i32).map(|v| v as i64)
            .or_else(|| doc.get("rating").and_then(Bson::as_i64));
        let comment = get_string(&doc, "comment");
        let created_at = doc.get("createdAt").and_then(iso_from_bson);

        item.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("userName", match user_name { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("rating", match rating { Some(v) => Bson::Int64(v), None => Bson::Null });
        item.insert("comment", match comment { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("createdAt", match created_at { Some(v) => Bson::String(v), None => Bson::Null });
        items.push(Bson::Document(item));
    }

    // ratings stored on orders
    let orders = db.collection::<Document>("orders");
    let mut order_cursor = orders.find(doc! {
        "restaurantId": &id,
        "rating": { "$exists": true }
    }).await.map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Find error: {}", e)))?;

    while let Some(doc) = order_cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Cursor error: {}", e)))? {
        let rating_doc = doc.get("rating").and_then(Bson::as_document);
        let score = rating_doc.and_then(|d| match d.get("score") {
            Some(Bson::Int32(v)) => Some(*v as i64),
            Some(Bson::Int64(v)) => Some(*v),
            Some(Bson::Double(v)) => Some(v.round() as i64),
            _ => None,
        });
        if score.is_none() {
            continue;
        }
        let comment = rating_doc.and_then(|d| d.get("comment")).and_then(Bson::as_str).map(|s| s.to_string());
        let created_at = doc.get("placedAt").and_then(iso_from_bson).or_else(|| doc.get("createdAt").and_then(iso_from_bson));
        let user_name = doc.get("customer")
            .and_then(Bson::as_document)
            .and_then(|c| c.get("name"))
            .and_then(Bson::as_str)
            .map(|s| s.to_string());
        let mut item = Document::new();
        item.insert("id", Bson::String(document_id(&doc).unwrap_or_default()));
        item.insert("userName", match user_name { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("rating", Bson::Int64(score.unwrap_or(0)));
        item.insert("comment", match comment { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("createdAt", match created_at { Some(v) => Bson::String(v), None => Bson::Null });
        items.push(Bson::Document(item));
    }

    Ok(data_response(Bson::Array(items)))
}

pub fn home_page_router(db: Database) -> Router{
    Router::new()
        .route("/", get(list_restaurants))
        .route("/{id}", get(get_restaurant_by_id))
        .route("/{id}/reviews", get(list_reviews))
        .with_state(db)
}
