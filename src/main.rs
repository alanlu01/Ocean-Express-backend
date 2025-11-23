use tokio::net::TcpListener;
use mongodb::{options::{ClientOptions, ServerApi, ServerApiVersion}, Client, Database};
use std::env;
use axum::Router;
use dotenv::dotenv;

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

    let app: Router = lib_app(db);

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
