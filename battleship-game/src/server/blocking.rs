// copyright 2020 Remi Bernotavicius

use super::GameServer;
use crate::protocol::Request;
use crossbeam_utils::thread;
use log::info;
use serde::Deserialize as _;
use std::sync::Mutex;
use std::{io, net};

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

pub trait Listener<'a>: Sync {
    type Stream: io::Read + io::Write + Send;
    type Incoming: Iterator<Item = io::Result<Self::Stream>> + 'a;

    fn incoming(&'a self) -> Self::Incoming;
}

impl<'a> Listener<'a> for net::TcpListener {
    type Incoming = net::Incoming<'a>;
    type Stream = net::TcpStream;

    fn incoming(&'a self) -> Self::Incoming {
        net::TcpListener::incoming(self)
    }
}

pub struct BlockingGameServer {
    game: Mutex<GameServer>,
}

impl BlockingGameServer {
    pub fn new() -> Self {
        Self {
            game: Mutex::new(GameServer::new()),
        }
    }

    pub fn process_requests<S: io::Read + io::Write>(&self, mut conn: S) {
        loop {
            match Request::deserialize(&mut serde_json::Deserializer::from_reader(&mut conn)) {
                Ok(request) => {
                    let reader = self.game.lock().unwrap().handle_request(request);
                    let response = reader.recv().unwrap();
                    serde_json::to_writer(&mut conn, &response).ok();
                }
                Err(e) => {
                    info!("abandoning connection due to error: {}", e);
                    break;
                }
            }
        }
    }

    pub fn run<'a, L: Listener<'a>>(&mut self, listener: &'a L) {
        thread::scope(|scope| {
            for connection in listener.incoming() {
                if let Ok(connection) = connection {
                    let their_self = &self;
                    scope.spawn(move |_| {
                        info!("received connection");
                        their_self.process_requests(connection);
                    });
                }
            }
        })
        .unwrap();
    }
}
