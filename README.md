# Ocean Express Backend

Rust/Axum backend for the Ocean Express delivery app. Provides auth, restaurants, menus, orders, delivery tasks, ratings, and push registration.

## Prerequisites
- Rust toolchain (1.72+ recommended)
- MongoDB connection string in `.env` (`MONGODB_URI=...`)
- `JWT_SECRET` set for token signing

## Run locally
```bash
cargo run
# Ocean-Express-backend
