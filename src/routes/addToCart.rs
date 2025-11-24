use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use futures::stream::TryStreamExt;
use mongodb::{bson::{doc, Bson, Document}, Database};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct DishInput {
    name: String,
    price: u32,
    quantity: u32,
    rice: String,
}

#[derive(Debug, Deserialize)]
struct AddToCartRequest {
    shop_id: String,
    user_id: String,
    dish: Vec<DishInput>,
}

async fn add_to_cart(
    State(db): State<Database>,
    Json(payload): Json<AddToCartRequest>,
) -> Result<Json<Document>, (StatusCode, String)>{
    let coll = db.collection::<Document>("cart");
    let menu_coll = db.collection::<Document>("menu");

    let filter = doc! { "shop_id": &payload.shop_id, "user_id": &payload.user_id };

    let mut cursor = coll.find(filter.clone())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let existing = cursor.try_next()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(mut cart_doc) = existing {
        // Extract existing dishes (name, rice)
        let mut dishes: Vec<Bson> = match cart_doc.get("dish") {
            Some(Bson::Array(arr)) => arr.clone(),
            _ => Vec::new(),
        };

        for dish in &payload.dish {
            // Find index of matching dish by name + rice
            let mut found = false;
            for elem in dishes.iter_mut() {
                if let Some(d) = elem.as_document_mut() {
                    let name_match = d.get("name").and_then(Bson::as_str).map(|s| s == dish.name).unwrap_or(false);
                    let rice_match = d.get("rice").and_then(Bson::as_str).map(|s| s == dish.rice).unwrap_or(false);
                    if name_match && rice_match {
                        // update quantity and price
                        let cur_q = d.get("quantity").and_then(Bson::as_i64).unwrap_or(0);
                        let new_q = cur_q + dish.quantity as i64;
                        d.insert("quantity", Bson::Int64(new_q));
                        d.insert("price", Bson::Int64(dish.price as i64));
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                // Validate that the dish exists in the `menu` collection for this shop
                let menu_filter = doc! { "shop_id": &payload.shop_id, "name": &dish.name };
                let menu_item = menu_coll.find_one(menu_filter,).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                if menu_item.is_none() {
                    return Err((StatusCode::BAD_REQUEST, format!("dish '{}' not available in shop {}'s menu", dish.name, payload.shop_id)));
                }

                let new_elem = doc! {
                    "name": &dish.name,
                    "price": dish.price as i64,
                    "quantity": dish.quantity as i64,
                    "rice": &dish.rice,
                };
                dishes.push(Bson::Document(new_elem));
            }
        }

        cart_doc.insert("dish", Bson::Array(dishes));

        coll.replace_one(filter.clone(), cart_doc.clone())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        // Create new cart document
        let mut dishes_bson = Vec::with_capacity(payload.dish.len());
        for dish in &payload.dish {
            // check if the dish exists in the 'menu' with repecting shop_id and name
            let menu_filter = doc! { "shop_id": &payload.shop_id, "name": &dish.name };
            let menu_item = menu_coll.find_one(menu_filter,).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if menu_item.is_none() {
                return Err((StatusCode::BAD_REQUEST, format!("dish '{}' not available in shop {}'s menu", dish.name, payload.shop_id)));
            }
            dishes_bson.push(Bson::Document(doc! {
                "name": &dish.name,
                "price": dish.price as i64,
                "quantity": dish.quantity as i64,
                "rice": &dish.rice,
            }));
        }

        let new_cart = doc! {
            "shop_id": &payload.shop_id,
            "user_id": &payload.user_id,
            "dishes": Bson::Array(dishes_bson),
        };

        coll.insert_one(new_cart)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // Return the updated/created cart document
    let mut cursor2 = coll.find(filter)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(doc) = cursor2.try_next()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
        Ok(Json(doc))
    } else {
        Err((StatusCode::NOT_FOUND, "cart not found after update".to_string()))
    }
}

pub fn add_to_cart_router(db: Database) -> Router{
    Router::new()
        .route("/", post(add_to_cart))
        .with_state(db)
}
