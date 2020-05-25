// copyright 2020 Remi Bernotavicius
use super::protocol::{Request, Response};
use super::{AttackResult, Game, Location, Play as _, PlayerId};
use log::info;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

pub mod blocking;

pub struct GameServer {
    game: Game,
    waiters: HashMap<PlayerId, Sender<Response>>,
    last_attack_result: Option<(Location, AttackResult)>,
}

impl GameServer {
    pub fn new() -> Self {
        Self {
            game: Game::new(),
            waiters: HashMap::new(),
            last_attack_result: None,
        }
    }

    pub fn handle_request(&mut self, request: Request) -> Receiver<Response> {
        info!("{:#?}", &request);
        let (sender, receiver) = channel();
        let response = match request {
            Request::AddPlayer(name) => self.game.add_player(&name).map(Response::AddPlayer).into(),
            Request::PlaceShip(player_id, ship_id, location, direction) => self
                .game
                .place_ship(player_id, ship_id, location, direction)
                .map(|()| Response::PlaceShip(ship_id, location, direction))
                .into(),
            Request::Advance(player_a_id, player_b_id, location) => {
                let result = self.game.advance(player_a_id, player_b_id, location);

                if let Ok(result) = &result {
                    self.last_attack_result = Some((location, result.clone()));
                }

                result.map(|r| Response::Advance(location, r)).into()
            }
            Request::WaitForTurn(player_id) => {
                if Some(player_id) == self.game.current_turn() {
                    let players = self.game.get_players();
                    let players = players.into_iter().filter(|&p| p != player_id).collect();
                    Response::WaitForTurn(self.last_attack_result.take(), players)
                } else {
                    self.waiters.insert(player_id, sender);
                    return receiver;
                }
            }
            Request::Winner => Response::Winner(self.game.winner()),
        };
        info!("{:#?}", &response);
        sender.send(response).unwrap();

        if let Some(player_id) = self.game.current_turn() {
            if let Some(sender) = self.waiters.remove(&player_id) {
                let players = self.game.get_players();
                let players = players.into_iter().filter(|&p| p != player_id).collect();
                let response = Response::WaitForTurn(self.last_attack_result.take(), players);
                info!("{:#?}", &response);
                sender.send(response).unwrap();
            }
        }
        receiver
    }
}
