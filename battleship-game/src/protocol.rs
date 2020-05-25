use super::{AttackResult, Direction, GameId, Location, PlayerId, ShipId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    AddPlayer(GameId, String),
    CreateGame,
    PlaceShip(PlayerId, ShipId, Location, Direction),
    Advance(PlayerId, PlayerId, Location),
    WaitForTurn(PlayerId),
    Winner(GameId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    AddPlayer(PlayerId),
    CreateGame(GameId),
    Advance(Location, AttackResult),
    PlaceShip(ShipId, Location, Direction),
    WaitForTurn(Option<(Location, AttackResult)>, Vec<PlayerId>),
    Winner(Option<PlayerId>),
    Error(super::Error),
}

impl From<super::Result<Self>> for Response {
    fn from(result: super::Result<Self>) -> Self {
        match result {
            Err(e) => Self::Error(e),
            Ok(r) => r,
        }
    }
}
