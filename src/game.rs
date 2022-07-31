use std::sync::Arc;

use base64::encode;
use md5::{Digest, Md5};
use rand::{Rng, SeedableRng};
use redis::Client as RedisClient;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

const MAX_GUESS: usize = 64;
const PASSWORD_LENGTH: usize = 8;
const SALT_LENGTH: usize = 8;
const GAME_EXPIRE: usize = 60 * 60 * 24;

#[derive(Serialize, Deserialize, Debug)]
pub struct GameInfo {
    pub salt: String,
    pub guess_count: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GameCreationInfo {
    pub salt: String,
    pub guess_count: usize,
    pub id: Uuid,
}

pub async fn get_game_info(redis: Arc<RedisClient>, game_id: Uuid) -> Result<GameInfo, AppError> {
    let mut conn = redis
        .get_async_connection()
        .await
        .map_err(|_| AppError::InternalServerError)?;
    let (guess_count, salt): (Option<usize>, Option<String>) = redis::pipe()
        .get(format!("game:{}:guess_count", game_id))
        .get(format!("game:{}:salt", game_id))
        .query_async(&mut conn)
        .await
        .map_err(|_| AppError::InternalServerError)?;
    if let (Some(guess_count), Some(salt)) = (guess_count, salt) {
        Ok(GameInfo { salt, guess_count })
    } else {
        Err(AppError::GameNotFound)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GuessResult {
    pub hash: String,
    pub guess: Vec<Match>,
    pub key: Option<String>,
}

pub async fn create_game(redis: Arc<RedisClient>) -> Result<GameCreationInfo, AppError> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    let salt: String = String::from_utf8(
        (0..SALT_LENGTH)
            .map(|_| rng.sample(&rand::distributions::Alphanumeric))
            .collect(),
    )
    .unwrap();
    let password: String = String::from_utf8(
        (0..PASSWORD_LENGTH)
            .map(|_| rng.sample(&rand::distributions::Alphanumeric))
            .collect(),
    )
    .unwrap();
    let mut hasher = Md5::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    let password = encode(hasher.finalize());
    let uuid = Uuid::from_bytes(rng.gen());

    let mut conn = redis
        .get_async_connection()
        .await
        .map_err(|_| AppError::InternalServerError)?;

    redis::pipe()
        .set_ex(format!("game:{}:guess_count", uuid), 0usize, GAME_EXPIRE)
        .set_ex(format!("game:{}:salt", uuid), &salt, GAME_EXPIRE)
        .set_ex(format!("game:{}:password", uuid), &password, GAME_EXPIRE)
        .query_async(&mut conn)
        .await
        .map_err(|_| AppError::InternalServerError)?;

    Ok(GameCreationInfo {
        salt,
        guess_count: 0,
        id: uuid,
    })
}

pub async fn make_guess(
    redis: Arc<RedisClient>,
    game_id: Uuid,
    guess: String,
) -> Result<GuessResult, AppError> {
    if guess.len() != PASSWORD_LENGTH {
        return Err(AppError::BadRequest);
    }
    let mut conn = redis
        .get_async_connection()
        .await
        .map_err(|_| AppError::InternalServerError)?;
    let (guess_count, salt, password): (Option<usize>, Option<String>, Option<String>) =
        redis::pipe()
            .incr(format!("game:{}:guess_count", game_id), 1)
            .get(format!("game:{}:salt", game_id))
            .get(format!("game:{}:password", game_id))
            .query_async(&mut conn)
            .await
            .map_err(|_| AppError::InternalServerError)?;
    if let (Some(guess_count), Some(salt), Some(password)) = (guess_count, salt, password) {
        if guess_count > MAX_GUESS {
            redis::pipe()
                .del(format!("game:{}:guess_count", game_id))
                .del(format!("game:{}:salt", game_id))
                .del(format!("game:{}:password", game_id))
                .query_async(&mut conn)
                .await
                .map_err(|_| AppError::InternalServerError)?;
            Err(AppError::GameNotFound)
        } else {
            let mut hasher = Md5::new();
            hasher.update(guess);
            hasher.update(salt);
            let guess = encode(hasher.finalize());
            if guess.len() != password.len() {
                return Err(AppError::BadRequest);
            }
            Ok(check_guess(guess, password))
        }
    } else {
        Err(AppError::GameNotFound)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
/// Represents a match for a given letter against the solution
pub enum Match {
    /// Letter is in the correct position
    Exact,
    /// Letter is in the solution but not in the correct position
    Close,
    /// Letter is not in the solution
    Wrong,
}

fn check_guess(input: String, solution: String) -> GuessResult {
    assert_eq!(input.len(), solution.len());

    let input_str = input;
    let input = input_str.as_bytes();
    let mut solution = solution.into_bytes();

    let mut diff = std::iter::repeat(Match::Wrong)
        .take(input.len())
        .collect::<Vec<_>>();

    // find exact matches first
    for (i, &b) in input.iter().enumerate() {
        if solution[i] == b {
            solution[i] = 0; // letters only match once
            diff[i] = Match::Exact;
        }
    }

    // now, find amber matches
    for (i, &b) in input.iter().enumerate() {
        if diff[i] != Match::Wrong {
            continue;
        }
        if let Some(j) = solution.iter().position(|&x| x == b) {
            solution[j] = 0; // letters only match once
            diff[i] = Match::Close;
        }
    }
    let key = if diff.iter().all(|value| value == &Match::Exact) {
        Some("31abhtykwu".into())
    } else {
        None
    };
    GuessResult {
        hash: input_str,
        guess: diff,
        key,
    }
}
