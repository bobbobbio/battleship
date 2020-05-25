use super::protocol::{Request, Response};
use super::{AttackResult, Direction, Error, Location, Player, PlayerId, Result, ShipId};

pub mod blocking;

pub enum ClientResponse {
    Attack(AttackResult),
    Winner(Option<PlayerId>),
    None,
}

pub struct GameClient {
    player: Option<Player>,
    player_id: Option<PlayerId>,
    other_players: Vec<PlayerId>,
}

impl GameClient {
    pub fn new() -> Self {
        Self {
            player: None,
            player_id: None,
            other_players: vec![],
        }
    }

    pub fn add_player(&mut self, name: &str) -> Request {
        self.player = Some(Player::new(name));
        Request::AddPlayer(name.into())
    }

    pub fn player(&mut self) -> Result<&mut Player> {
        if let Some(player) = self.player.as_mut() {
            Ok(player)
        } else {
            Err(Error::CommunicationError)
        }
    }

    pub fn handle_response(&mut self, response: Response) -> Result<ClientResponse> {
        match response {
            Response::AddPlayer(id) => {
                self.player_id = Some(id);
                Ok(ClientResponse::None)
            }
            Response::Advance(location, result) => {
                if result.is_hit() {
                    self.player()?.speculative_field.record_hit(location)?;
                } else {
                    self.player()?.speculative_field.record_miss(location)?;
                }
                Ok(ClientResponse::Attack(result))
            }
            Response::Error(error) => Err(error),
            Response::WaitForTurn(result, players) => {
                self.other_players = players;
                if let Some((location, result)) = result {
                    if result.is_hit() {
                        self.player()?.own_field.record_hit(location)?;
                    } else {
                        self.player()?.own_field.record_miss(location)?;
                    }
                    Ok(ClientResponse::Attack(result))
                } else {
                    Ok(ClientResponse::None)
                }
            }
            Response::PlaceShip(ship_id, location, direction) => {
                self.player()?.place_ship(ship_id, location, direction)?;
                Ok(ClientResponse::None)
            }
            Response::Winner(player) => Ok(ClientResponse::Winner(player)),
        }
    }

    pub fn advance(
        &self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
        guess: Location,
    ) -> Request {
        Request::Advance(player_a_id, player_b_id, guess)
    }

    pub fn place_ship(
        &self,
        player_id: PlayerId,
        ship_id: ShipId,
        location: Location,
        direction: Direction,
    ) -> Request {
        Request::PlaceShip(player_id, ship_id, location, direction)
    }

    pub fn wait_for_turn(&self) -> Request {
        Request::WaitForTurn(self.player_id.unwrap())
    }

    pub fn winner(&self) -> Request {
        Request::Winner
    }

    pub fn get_player(&self, player_id: PlayerId) -> Result<&Player> {
        if self.player_id == Some(player_id) && self.player.is_some() {
            Ok(self.player.as_ref().unwrap())
        } else {
            Err(Error::UnknownPlayer(player_id))
        }
    }

    pub fn other_player_ids(&self) -> Vec<PlayerId> {
        self.other_players.clone()
    }

    pub fn player_id(&self) -> PlayerId {
        self.player_id.unwrap()
    }
}
