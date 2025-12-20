use axum::{Router, extract::{State, Path}, routing::get, response::IntoResponse, Json};
use mongodb::{bson::{doc, Bson, Document}, Database};
use futures::stream::TryStreamExt;
use axum::http::StatusCode;
use crate::routes::common::{ApiResult, error_response, get_string, get_bool, get_i64, get_array, document_id};

async fn get_menu(Path(shop_id): Path<String>, State(db): State<Database>) -> ApiResult{
    // Query the `menu` collection
    let collection = db.collection::<Document>("menu");

    // filter: match the shop_id as a string
    let filter = doc! {
        "$or": [
            { "shop_id": shop_id.clone() },
            { "restaurantId": shop_id.clone() },
            { "restaurant_id": shop_id.clone() }
        ]
    };

    println!("menu.get_menu - using filter: {:?}", filter);

    let mut cursor = collection.find(filter)
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Find error: {}", e)))?;
    let mut results: Vec<Bson> = Vec::new();
    while let Some(doc) = cursor.try_next()
        .await
        .map_err(|e| error_response(StatusCode::INTERNAL_SERVER_ERROR, "server.error", &format!("Cursor error: {}", e)))? {
        let mut item = Document::new();
        let id = document_id(&doc);
        let name = get_string(&doc, "name");
        let description = get_string(&doc, "description");
        let price = match doc.get("price") {
            Some(Bson::Int32(v)) => *v as i64,
            Some(Bson::Int64(v)) => *v,
            Some(Bson::Double(v)) => v.round() as i64,
            Some(Bson::String(s)) => s.parse::<i64>().unwrap_or(0),
            _ => 0,
        };
        let sizes = get_array(&doc, "sizes").or_else(|| get_array(&doc, "size"));
        let spiciness = get_array(&doc, "spicinessOptions");
        let image = get_string(&doc, "imageUrl");
        let is_available = get_bool(&doc, "isAvailable").unwrap_or(true);
        let sort_order = get_i64(&doc, "sortOrder").unwrap_or(0);
        let allergens = get_array(&doc, "allergens").unwrap_or_default();
        let tags = get_array(&doc, "tags").unwrap_or_default();

        item.insert("id", match id { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("name", match name { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("description", match description { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("price", Bson::Int64(price));
        item.insert("sizes", match sizes { Some(v) => Bson::Array(v), None => Bson::Null });
        item.insert("spicinessOptions", match spiciness { Some(v) => Bson::Array(v), None => Bson::Null });
        item.insert("imageUrl", match image { Some(v) => Bson::String(v), None => Bson::Null });
        item.insert("isAvailable", Bson::Boolean(is_available));
        item.insert("sortOrder", Bson::Int64(sort_order));
        item.insert("allergens", Bson::Array(allergens));
        item.insert("tags", Bson::Array(tags));
        results.push(Bson::Document(item));
    }

    println!("menu.get_menu - found {} documents", results.len());

    // Return both the primary shape (`data.items`) and a lenient `items` for clients that use the fallback.
    let items = Bson::Array(results);
    let body = doc! {
        "data": { "items": items.clone() },
        "items": items.clone(),
    };

    Ok(Json(body).into_response())
}

pub fn menu_router(db: Database) -> Router{
    Router::new()
        .route("/{shop_id}/menu", get(get_menu))
        .with_state(db)
}

// {
// "items":
// [ {
//     "name": "Burger",
//     "description": "...",
//     "price": 180,
//     "sizes": ["Regular", "Large"],
//     "spicinessOptions": ["Mild","Medium","Hot"],
//     "imageUrl": null
//  } ]
// }
