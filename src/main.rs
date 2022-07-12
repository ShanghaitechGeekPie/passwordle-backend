use std::sync::Arc;

use axum::extract::{Extension, Path};
use axum::routing::{get, post};
use axum::{Json, Router};
// We prefer to keep `main.rs` and `lib.rs` separate as it makes it easier to add extra helper
// binaries later which share code with the main project. It could save you from a nontrivial
// refactoring effort in the future.
//
// Whether to make `main.rs` just a thin shim that awaits a `run()` function in `lib.rs`, or
// to put the application bootstrap logic here is an open question. Both approaches have their
// upsides and their downsides. Your input is welcome!
use clap::Parser;
use redis::Client as RedisClient;
use uuid::Uuid;

use passwordle::config::Config;
use passwordle::error::AppError;
use passwordle::game::{
    create_game, get_game_info, make_guess, GameCreationInfo, GameInfo, GuessResult,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    let config: Config = Config::parse();
    let client = RedisClient::open(config.redis_url)?;
    let client = Arc::new(client);

    let app = Router::new()
        .route("/api/games/:id", get(show_game_status))
        .route("/api/guess/:id/", post(guess_post))
        .route("/api/create", post(game_create_post))
        .layer(Extension(client));

    axum::Server::bind(&config.bind_url.parse()?)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

/// Handler for `GET /games/:id`.
async fn show_game_status(
    Path(game_id): Path<Uuid>,
    Extension(client): Extension<Arc<RedisClient>>,
) -> Result<Json<GameInfo>, AppError> {
    let info = get_game_info(client, game_id).await?;
    Ok(info.into())
}

/// Handler for `POST /create/`.
async fn game_create_post(
    Extension(client): Extension<Arc<RedisClient>>,
) -> Result<Json<GameCreationInfo>, AppError> {
    let info = create_game(client).await?;
    Ok(info.into())
}

#[derive(Debug, serde::Deserialize)]
struct MakeGuessRequest {
    guess: String,
}

/// Handler for `POST /games/:id/guess`.
async fn guess_post(
    Path(game_id): Path<Uuid>,
    Json(payload): Json<MakeGuessRequest>,
    Extension(client): Extension<Arc<RedisClient>>,
) -> Result<Json<GuessResult>, AppError> {
    let info = make_guess(client, game_id, payload.guess).await?;
    Ok(info.into())
}
