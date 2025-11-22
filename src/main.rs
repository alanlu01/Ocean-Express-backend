use axum::Router;
use hyper::StatusCode;
use mongodb::{Client, options::ClientOptions};
use std::net::SocketAddr;

// call the library crate's `app` function exported from `src/lib.rs`
// Not "mod lib", because main is the root of the binary crate,
// if use "mod lib" here, the routes in lib.rs will not be found
use Expressing_server::app as lib_app;

#[tokio::main]
async fn main()
 {
    // initialize mongodb client and connect to database
    let client_options = ClientOptions::parse("mongodb://localhost:27017")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("parse error: {}", e)
            )
        }).unwrap();
    let client = Client::with_options(client_options).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("client error: {}", e)
        )
    }).unwrap();

    let db = client.database("Local_ExpressingDB");

    // pass db to app -> routes -> handlers
    let app: Router = lib_app(db);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
