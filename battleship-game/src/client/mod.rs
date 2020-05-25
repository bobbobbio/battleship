use super::protocol::{Request, Response};
use super::{AttackResult, Direction, Error, GameId, Location, Player, PlayerId, Result, ShipId};

pub mod blocking;

pub enum ClientResponse {
    Attack(AttackResult),
    Winner(Option<PlayerId>),
    None,
}

pub struct GameClient {
    game_id: Option<GameId>,
    player: Option<Player>,
    player_id: Option<PlayerId>,
    other_players: Vec<PlayerId>,
}

impl GameClient {
    pub fn new() -> Self {
        Self {
            game_id: None,
            player: None,
            player_id: None,
            other_players: vec![],
        }
    }

    pub fn create_game(&mut self) -> Request {
        Request::CreateGame
    }

    pub fn join_game(&mut self, game_id: GameId) {
        self.game_id = Some(game_id);
    }

    pub fn add_player(&mut self, name: &str) -> Request {
        self.player = Some(Player::new(name));
        Request::AddPlayer(self.game_id.unwrap(), name.into())
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
            Response::CreateGame(game_id) => {
                self.join_game(game_id);
                Ok(ClientResponse::None)
            }
            Response::Winner(player_id) => Ok(ClientResponse::Winner(player_id)),
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

    pub fn get_player(&self, player_id: PlayerId) -> Result<&Player> {
        if self.player_id == Some(player_id) && self.player.is_some() {
            Ok(self.player.as_ref().unwrap())
        } else {
            Err(Error::UnknownPlayer(player_id))
        }
    }

    pub fn winner(&self) -> Request {
        Request::Winner(self.game_id.unwrap())
    }

    pub fn other_player_ids(&self) -> Vec<PlayerId> {
        self.other_players.clone()
    }

    pub fn player_id(&self) -> PlayerId {
        self.player_id.unwrap()
    }

    pub fn game_id(&self) -> GameId {
        self.game_id.unwrap()
    }
}
