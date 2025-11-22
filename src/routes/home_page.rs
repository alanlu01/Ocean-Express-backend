use axum::{Router, http::StatusCode, routing::get, extract::State, Json};
use mongodb::{bson::doc, bson::Document, Database};
use futures::stream::TryStreamExt;

async fn get_all_shops(State(db): State<Database>) -> Result<Json<Vec<Document>>, (StatusCode, String)>{
    let collections = db.list_collection_names().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("List collections error: {}", e)))?;
    if collections.is_empty() {
        return Ok(Json(Vec::new()));
    }
    let first_coll = &collections[0];
    let collection = db.collection::<Document>(first_coll);
    let mut cursor = collection.find(doc! {}).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Find error: {}", e)))?;
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cursor error: {}", e)))? {
        results.push(doc);
    }
    Ok(Json(results))
}

pub fn home_page_router(db: Database) -> Router{
    Router::new()
        .route("/", get(get_all_shops))
        .with_state(db)
}
