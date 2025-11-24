use axum::{Router, http::StatusCode, routing::get, extract::State, Json};
use mongodb::{bson::doc, bson::Document, Database};
use futures::stream::TryStreamExt;

async fn get_all_shops(State(db): State<Database>) -> Result<Json<Vec<Document>>, (StatusCode, String)>{
    let collections = db.list_collection_names()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("List collections error: {}", e)))?;
    println!("collections in DB: {:?}", collections);

    if collections.is_empty() {
        println!("no collections found in database");
        return Ok(Json(Vec::new()));
    }

    let coll_name = if collections.iter().any(|c| c == "shops") {
        "shops"
    } else {
        // db : 0 cart, 1 menu, 2 shops
        &collections[2]
    };

    // test print
    println!("using collection: {}", coll_name);
    let collection = db.collection::<Document>(coll_name);
    let mut cursor = collection
        .find(doc! {})
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("home_page error: {}", e)))?;
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cursor error: {}", e)))? {
        results.push(doc);
    }
    println!("found {} documents", results.len());
    Ok(Json(results))
}

pub fn home_page_router(db: Database) -> Router{
    Router::new()
        .route("/", get(get_all_shops))
        .with_state(db)
}
