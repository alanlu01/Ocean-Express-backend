use axum::{Router, http::StatusCode, routing::get, extract::{State, Path}, Json};
use mongodb::{bson::{doc, Bson, Document}, Database};
use futures::stream::TryStreamExt;

// GET /restaurants
async fn list_restaurants(State(db): State<Database>) -> Result<Json<Document>, (StatusCode, String)>{
    let collections = db.list_collection_names()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("List collections error: {}", e)))?;

    if collections.is_empty() {
        let empty = doc! { "data": Bson::Array(Vec::new()) };
        return Ok(Json(empty));
    }

    let coll_name = if collections.iter().any(|c| c == "shops") {
        "shops"
    } else {
        &collections[2]
    };

    let collection = db.collection::<Document>(coll_name);
    let mut cursor = collection.find(doc! {})
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Find error: {}", e)))?;

    let mut items: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cursor error: {}", e)))? {
        // mapping attributes
        let id = doc.get("id")
            .and_then(Bson::as_str)
            .map(|s| s.to_string());
        let name = doc.get("name")
            .and_then(Bson::as_str)
            .map(|s| s.to_string());
        let image = doc.get("imageUrl")
            .and_then(Bson::as_str)
            .map(|s| s.to_string());

        let mut item = Document::new();
        item.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("name", match name { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("imageUrl", match image { Some(v) => Bson::String(v), None => Bson::Null });

        items.push(Bson::Document(item));
    }

    let res = doc! { "data": Bson::Array(items) };
    Ok(Json(res))
}

// GET /restaurants/{id}
async fn get_restaurant_by_id(Path(id): Path<String>, State(db): State<Database>) -> Result<Json<Document>, (StatusCode, String)>{
    let collection = db.collection::<Document>("shops");

    // matching by id
    let filter = doc! { "id": &id };

    let found = collection.find_one(filter)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    match found {
        Some(doc) => {
            let id = doc.get("id")
                .and_then(Bson::as_str)
                .map(|s| s.to_string());

            let name = doc.get("name").and_then(Bson::as_str).map(|s| s.to_string());
            let image = doc.get("imageUrl").and_then(Bson::as_str).map(|s| s.to_string());
            let address = doc.get("address").and_then(Bson::as_str).map(|s| s.to_string());
            let phone = doc.get("phone").and_then(Bson::as_str).map(|s| s.to_string());

            let mut out = Document::new();
            let mut body = Document::new();
            body.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("name", match name { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("imageUrl", match image { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("address", match address { Some(v) => Bson::String(v), None => Bson::Null });
            body.insert("phone", match phone { Some(v) => Bson::String(v), None => Bson::Null });

            out.insert("data", Bson::Document(body));
            Ok(Json(out))
        }
        None => {
            let out = doc! { "data": Bson::Null };
            Ok(Json(out))
        }
    }
}

pub fn home_page_router(db: Database) -> Router{
    Router::new()
        .route("/", get(list_restaurants))
        .route("/{id}", get(get_restaurant_by_id))
        .with_state(db)
}
