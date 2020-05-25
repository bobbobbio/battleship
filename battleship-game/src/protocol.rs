use super::{AttackResult, Direction, Location, PlayerId, ShipId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    AddPlayer(String),
    PlaceShip(PlayerId, ShipId, Location, Direction),
    Advance(PlayerId, PlayerId, Location),
    WaitForTurn(PlayerId),
    Winner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    AddPlayer(PlayerId),
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
