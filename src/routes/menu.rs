use axum::{Router, extract::{State, Path}, routing::get, Json};
use mongodb::{bson::{doc, Document, oid::ObjectId}, Database};
use futures::stream::TryStreamExt;
use axum::http::StatusCode;

async fn get_menu(Path(shop_id): Path<String>, State(db): State<Database>) -> Result<Json<Vec<Document>>, (StatusCode, String)>{
    // Query the `menu` collection
    let collection = db.collection::<Document>("menu");

    // filter: match the shop_id as oid or string
    let filter = if let Ok(oid) = ObjectId::parse_str(&shop_id) {
        doc! {"shop_id": oid.clone()}
    } else {
        doc! {"shop_id": shop_id.clone()}
    };

    println!("menu.get_menu - using filter: {:?}", filter);

    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Find error: {}", e)))?;
    let mut results: Vec<Document> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Cursor error: {}", e)))? {
        results.push(doc);
    }

    println!("menu.get_menu - found {} documents", results.len());

    Ok(Json(results))
}

pub fn menu_router(db: Database) -> Router{
    Router::new()
        .route("/{shop_id}/menu", get(get_menu))
        .with_state(db)
}
