#![allow(non_snake_case)]

use tokio::net::TcpListener;
use tokio::time::{sleep, Duration, interval};
use mongodb::{options::{ClientOptions, ServerApi, ServerApiVersion}, Client, Database, bson::{doc, DateTime}};
use std::env;
use axum::Router;
use dotenv::dotenv;
use chrono::Utc;
use reqwest::Client as HttpClient;

// import the app constructor from lib,
// don't use "mod lib", the compiler will find through src/lib/routes
use Expressing_server::app as lib_app;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
    dotenv().ok();

    let mongo_uri = env::var("MONGODB_URI").unwrap();

    let mut client_options = ClientOptions::parse(&mongo_uri).await?;
    let server_api = ServerApi::builder().version(ServerApiVersion::V1).build();
    client_options.server_api = Some(server_api);

    let client = Client::with_options(client_options)?;
    let db: Database = client.database("NTOUExpressingDB");

    let app: Router = lib_app(db.clone());

    // background task: auto cancel orders older than 1 hour if not delivered/cancelled
    let db_for_task = db.clone();
    tokio::spawn(async move {
        let orders = db_for_task.collection::<mongodb::bson::Document>("orders");
        loop {
            let cutoff = Utc::now().timestamp_millis() - 60 * 60 * 1000;
            let filter = doc! {
                "status": { "$nin": ["delivered", "cancelled"] },
                "placedAt": { "$lt": DateTime::from_millis(cutoff) }
            };
            let update = doc! {
                "$set": { "status": "cancelled" },
                "$push": { "statusHistory": { "status": "cancelled", "timestamp": DateTime::from_millis(Utc::now().timestamp_millis()) } }
            };
            match orders.update_many(filter, update).await {
                Ok(res) => {
                    if res.modified_count > 0 {
                        println!("Auto-cancelled {} stale orders (>1h).", res.modified_count);
                    }
                }
                Err(e) => {
                    eprintln!("Auto-cancel task error: {}", e);
                }
            }
            sleep(Duration::from_secs(60)).await;
        }
    });

    // self-ping to keep Render awake (optional: set SELF_PING_URL)
    if let Ok(self_url) = env::var("SELF_PING_URL") {
        tokio::spawn(async move {
            let client = HttpClient::new();
            let mut ticker = interval(Duration::from_secs(14 * 60));
            // first tick immediately
            loop {
                ticker.tick().await;
                match client.get(&self_url).send().await {
                    Ok(resp) => println!("self-ping ok {} {}", resp.status(), self_url),
                    Err(e) => eprintln!("self-ping failed: {}", e),
                }
            }
        });
    }

    // Start server
    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("listening on http://{}", listener.local_addr()?);

    //axum::Server::bind(&addr)
    //  .serve(app.into_make_service())
    //  .await?;
    // this is for axum 0.7 or older version
    axum::serve(listener, app)
        .await
        .unwrap();

    Ok(())
}
