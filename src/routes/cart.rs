use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use futures::TryStreamExt;
use mongodb::{Database, bson::{Document, doc}};

async fn get_cart(State(db): State<Database>) -> Result<Json<Vec<Document>>, (StatusCode, String)>{
    let collections = db.list_collection_names()
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("List collections error: {}", e)))?;
    println!("collections in DB: {:?}", collections);

    if collections.is_empty() {
        println!("no collections found in database");
        return Ok(Json(Vec::new()));
    }

    let coll_name = if collections.iter().any(|c| c == "cart") {
        "cart"
    } else {
        &collections[0]
    };

    // test print
    println!("using collection: {}", coll_name);
    let collection = db.collection::<Document>(coll_name);
    let mut cursor = collection
        .find(doc! {})
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("cart error: {}", e)))?;
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cursor error: {}", e)))? {
        results.push(doc);
    }
    Ok(Json(results))
}

pub fn cart_router(db: Database) -> Router{
    Router::new()
        .route("/", get(get_cart))
        .with_state(db)
}
