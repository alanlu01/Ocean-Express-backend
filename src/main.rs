use axum::Router;
use std::net::SocketAddr;

// call the library crate's `app` function exported from `src/lib.rs`
// Not "mod lib", because main is the root of the binary crate,
// if use "mod lib" here, the routes in lib.rs will not be found
use Expressing_server::app as lib_app;

#[tokio::main]
async fn main()
 {
    let app: Router = lib_app();

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
