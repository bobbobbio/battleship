// copyright 2020 Remi Bernotavicius

use super::{ClientResponse, GameClient};
use crate::protocol::Response;
use crate::{
    AttackResult, Direction, Error as GameError, GameId, Location, Play, Player, PlayerId,
    Result as GameResult, ShipId,
};
use serde::Deserialize;
use std::io;
use std::net::TcpStream;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Serde(serde_json::Error),
    Game(crate::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Self::Game(e)
    }
}

pub struct BlockingGameClient {
    game: GameClient,
    connection: TcpStream,
}

impl BlockingGameClient {
    pub fn new(mut connection: TcpStream, name: &str, game_id: Option<GameId>) -> Result<Self> {
        let mut game = GameClient::new();

        if let Some(game_id) = game_id {
            game.join_game(game_id);
        } else {
            serde_json::to_writer(&mut connection, &game.create_game())?;
            let mut de = serde_json::Deserializer::from_reader(&mut connection);
            game.handle_response(Response::deserialize(&mut de)?)?;
        }

        serde_json::to_writer(&mut connection, &game.add_player(name))?;

        let mut de = serde_json::Deserializer::from_reader(&mut connection);
        game.handle_response(Response::deserialize(&mut de)?)?;

        Ok(Self { game, connection })
    }

    pub fn wait_for_turn(&mut self) -> Result<Option<AttackResult>> {
        let request = self.game.wait_for_turn();
        serde_json::to_writer(&mut self.connection, &request)?;

        let mut de = serde_json::Deserializer::from_reader(&mut self.connection);
        let response = Response::deserialize(&mut de)?;
        if let ClientResponse::Attack(result) = self.game.handle_response(response)? {
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    pub fn other_player_ids(&self) -> Vec<PlayerId> {
        self.game.other_player_ids()
    }

    pub fn winner(&mut self) -> Result<Option<PlayerId>> {
        let request = self.game.winner();
        serde_json::to_writer(&mut self.connection, &request)?;

        let mut de = serde_json::Deserializer::from_reader(&mut self.connection);
        let response = Response::deserialize(&mut de)?;
        if let ClientResponse::Winner(player) = self.game.handle_response(response)? {
            Ok(player)
        } else {
            Err(Error::Game(GameError::CommunicationError))
        }
    }

    pub fn player_id(&self) -> PlayerId {
        self.game.player_id()
    }

    pub fn game_id(&self) -> GameId {
        self.game.game_id()
    }
}

impl Play for BlockingGameClient {
    fn advance(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
        guess: Location,
    ) -> GameResult<AttackResult> {
        let request = self.game.advance(player_a_id, player_b_id, guess);
        serde_json::to_writer(&mut self.connection, &request)
            .map_err(|_| GameError::CommunicationError)?;
        let mut de = serde_json::Deserializer::from_reader(&mut self.connection);
        let response = Response::deserialize(&mut de).map_err(|_| GameError::CommunicationError)?;
        if let ClientResponse::Attack(result) = self.game.handle_response(response)? {
            Ok(result)
        } else {
            Err(GameError::CommunicationError)
        }
    }

    fn advance_automatically(
        &mut self,
        _player_a_id: PlayerId,
        _player_b_id: PlayerId,
    ) -> GameResult<AttackResult> {
        unimplemented!()
    }

    fn place_ship(
        &mut self,
        player_id: PlayerId,
        ship: ShipId,
        location: Location,
        direction: Direction,
    ) -> GameResult<()> {
        let request = self.game.place_ship(player_id, ship, location, direction);
        serde_json::to_writer(&mut self.connection, &request)
            .map_err(|_| GameError::CommunicationError)?;
        let mut de = serde_json::Deserializer::from_reader(&mut self.connection);
        let response = Response::deserialize(&mut de).map_err(|_| GameError::CommunicationError)?;
        self.game.handle_response(response)?;
        Ok(())
    }

    fn get_player(&self, player_id: PlayerId) -> GameResult<&Player> {
        self.game.get_player(player_id)
    }
}
