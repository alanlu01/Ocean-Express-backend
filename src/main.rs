use axum::{Router, routing::get};
use std::net::SocketAddr;
use tokio::net::TcpListener; // Add this import
mod lib;

#[tokio::main]
async fn main() {
    let app: Router = lib::app();

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
