// Copyright 2020 Remi Bernotavicius

use battleship_game::server::blocking::{BlockingGameServer, Error as ServerError, Listener};
use log::info;
use std::{io, net, sync::Mutex};
use websocket::{
    server::{sync::AcceptResult, NoTlsAcceptor, WsServer},
    Message, OwnedMessage,
};

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Server(ServerError),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ServerError> for Error {
    fn from(e: ServerError) -> Self {
        Self::Server(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

struct WsListener(Mutex<WsServer<NoTlsAcceptor, net::TcpListener>>);

impl WsListener {
    fn bind<A: net::ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let s = WsServer::<NoTlsAcceptor, net::TcpListener>::bind(addr)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, ""))?;
        Ok(Self(Mutex::new(s)))
    }

    fn local_addr(&self) -> io::Result<net::SocketAddr> {
        self.0.lock().unwrap().local_addr()
    }

    fn accept(&self) -> AcceptResult<net::TcpStream> {
        self.0.lock().unwrap().accept()
    }
}

struct WsStream {
    stream: websocket::client::sync::Client<net::TcpStream>,
    buffer: Vec<u8>,
}

impl io::Read for WsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buffer.len() < buf.len() {
            let message = self
                .stream
                .recv_message()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, ""))?;
            match message {
                OwnedMessage::Binary(d) => self.buffer.extend(d),
                OwnedMessage::Text(d) => self.buffer.extend(d.bytes()),
                _ => (),
            }
        }

        let read = self.buffer.as_slice().read(buf)?;
        self.buffer = self.buffer.split_off(read);
        Ok(read)
    }
}

impl io::Write for WsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream
            .send_message(&Message::binary(buf))
            .map_err(|_| io::Error::new(io::ErrorKind::Other, ""))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.writer_mut().flush()
    }
}

struct WsIncoming<'a> {
    listener: &'a WsListener,
}

impl<'a> WsIncoming<'a> {
    fn accept(&mut self) -> io::Result<WsStream> {
        let upgrade = self
            .listener
            .accept()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, ""))?;
        let client = upgrade
            .accept()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, ""))?;
        Ok(WsStream {
            stream: client,
            buffer: vec![],
        })
    }
}

impl<'a> Iterator for WsIncoming<'a> {
    type Item = io::Result<WsStream>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.accept())
    }
}

impl<'a> Listener<'a> for WsListener {
    type Stream = WsStream;
    type Incoming = WsIncoming<'a>;

    fn incoming(&'a self) -> Self::Incoming {
        WsIncoming { listener: self }
    }
}

fn main() -> Result<()> {
    let arg = std::env::args().skip(1).next();

    simple_logger::init_with_level(log::Level::Info).unwrap();

    let listener = WsListener::bind(&arg.unwrap_or("0.0.0.0:0".into()))?;
    info!("listening on {}", listener.local_addr()?);

    let mut game_server = BlockingGameServer::new();
    game_server.run(&listener);
    Ok(())
}
