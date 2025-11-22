use axum::{Json, Router, http::StatusCode, routing::get};
use futures::stream::TryStreamExt;
use mongodb::{Client, bson::Document, bson::doc, options::ClientOptions};

async fn get_all_shops() -> Result<Json<Vec<Document>>, (StatusCode, String)>{
    let client_options = ClientOptions::parse("mongodb://localhost:27017")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Parse error: {}", e),
            )
        })?;
    let client = Client::with_options(client_options).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Client error: {}", e),
        )
    })?;

    // Access database
    let db = client.database("Local_ExpressingDB");

    // List collections to select shops
    let collections = db
        .list_collection_names()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("List collections error: {}", e)))?;

    // No collections found in the database
    if collections.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let first_coll = &collections[0];
    println!("using collection: {}", first_coll);

    let collection = db.collection::<Document>(first_coll);

    // Get all data in the selected collection (temporary, now for testing)
    let mut cursor = collection.find(doc! {})
        .await
        .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Find error: {}", e),
        )
    })?;

    // Collection in results
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Cursor error: {}", e),
        )
    })? {
        results.push(doc);
    }

    Ok(Json(results))
}

pub fn home_page_router() -> Router{
    Router::new().route("/", get(get_all_shops))
}
