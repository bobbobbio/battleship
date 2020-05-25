// copyright 2020 Remi Bernotavicius
use super::protocol::{Request, Response};
use super::{
    AttackResult, Direction, Error, Game, GameId, Location, Play as _, PlayerId, Result, ShipId,
};
use log::info;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

pub mod blocking;

pub struct GameServer {
    games: HashMap<GameId, Game>,
    waiters: HashMap<PlayerId, Sender<Response>>,
    last_attack_result: Option<(Location, AttackResult)>,
}

impl GameServer {
    pub fn new() -> Self {
        Self {
            games: HashMap::new(),
            waiters: HashMap::new(),
            last_attack_result: None,
        }
    }

    fn game(&mut self, game_id: GameId) -> Result<&mut Game> {
        self.games
            .get_mut(&game_id)
            .ok_or(Error::UnknownGame(game_id))
    }

    fn create_game(&mut self) -> GameId {
        let max_id = self.games.keys().cloned().max().unwrap_or(GameId(0));
        let id = max_id.incr();
        self.games.insert(id, Game::new(id));
        id
    }

    fn add_player(&mut self, game_id: GameId, name: &str) -> Result<PlayerId> {
        self.game(game_id)?.add_player(&name)
    }

    fn place_ship(
        &mut self,
        player_id: PlayerId,
        ship_id: ShipId,
        location: Location,
        direction: Direction,
    ) -> Result<()> {
        self.game(player_id.game_id())?
            .place_ship(player_id, ship_id, location, direction)
    }

    fn advance(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
        location: Location,
    ) -> Result<AttackResult> {
        let result = self
            .game(player_a_id.game_id())?
            .advance(player_a_id, player_b_id, location);

        if let Ok(result) = &result {
            self.last_attack_result = Some((location, result.clone()));
        }

        result
    }

    fn wait_for_turn(&mut self, player_id: PlayerId) -> Result<Option<Response>> {
        if Some(player_id) == self.game(player_id.game_id())?.current_turn() {
            let players = self.game(player_id.game_id())?.get_players();
            let players = players.into_iter().filter(|&p| p != player_id).collect();
            Ok(Some(Response::WaitForTurn(
                self.last_attack_result.take(),
                players,
            )))
        } else {
            Ok(None)
        }
    }

    fn check_waiters(&mut self) {
        let game_ids: Vec<_> = self.games.keys().cloned().collect();
        for game_id in game_ids {
            if let Some(player_id) = self.game(game_id).unwrap().current_turn() {
                if let Some(sender) = self.waiters.remove(&player_id) {
                    let players = self.game(game_id).unwrap().get_players();
                    let players = players.into_iter().filter(|&p| p != player_id).collect();
                    let response = Response::WaitForTurn(self.last_attack_result.take(), players);
                    info!("{:#?}", &response);
                    sender.send(response).unwrap();
                }
            }
        }
    }

    fn winner(&mut self, game_id: GameId) -> Result<Option<PlayerId>> {
        Ok(self.game(game_id)?.winner())
    }

    pub fn handle_request(&mut self, request: Request) -> Receiver<Response> {
        info!("{:#?}", &request);
        let (sender, receiver) = channel();
        let response = match request {
            Request::AddPlayer(game_id, name) => self
                .add_player(game_id, &name)
                .map(Response::AddPlayer)
                .into(),
            Request::PlaceShip(player_id, ship_id, location, direction) => self
                .place_ship(player_id, ship_id, location, direction)
                .map(|()| Response::PlaceShip(ship_id, location, direction))
                .into(),
            Request::Advance(player_a_id, player_b_id, location) => self
                .advance(player_a_id, player_b_id, location)
                .map(|r| Response::Advance(location, r))
                .into(),
            Request::WaitForTurn(player_id) => {
                let response = self.wait_for_turn(player_id);
                if let Ok(None) = &response {
                    self.waiters.insert(player_id, sender);
                    return receiver;
                } else {
                    response.map(|c| c.unwrap()).into()
                }
            }
            Request::Winner(game_id) => self.winner(game_id).map(Response::Winner).into(),
            Request::CreateGame => Response::CreateGame(self.create_game()),
        };
        info!("{:#?}", &response);
        sender.send(response).unwrap();

        self.check_waiters();

        receiver
    }
}
