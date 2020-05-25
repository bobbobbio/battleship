// Copyright 2020 Remi Bernotavicius

use battleship_game::{
    client::blocking::BlockingGameClient, row_to_letter, server::blocking::BlockingGameServer,
    BattleField, Cell, Direction, Game, Location, Play, Player, PlayerId, Ship, ShipId,
};
use log::info;
use std::collections::HashMap;
use std::io::{self, BufRead as _, Write as _};
use std::{fmt, net, str};

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Game(battleship_game::Error),
    ClientError(battleship_game::client::blocking::Error),
    ServerError(battleship_game::server::blocking::Error),
}

type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<battleship_game::Error> for Error {
    fn from(e: battleship_game::Error) -> Self {
        Self::Game(e)
    }
}

impl From<battleship_game::client::blocking::Error> for Error {
    fn from(e: battleship_game::client::blocking::Error) -> Self {
        Self::ClientError(e)
    }
}

impl From<battleship_game::server::blocking::Error> for Error {
    fn from(e: battleship_game::server::blocking::Error) -> Self {
        Self::ServerError(e)
    }
}

fn format_battlefield(ships: &HashMap<ShipId, Ship>, field: &BattleField) -> Vec<String> {
    let mut lines = vec![];

    let mut line = String::from(" ");
    for i in 1..=field.height() {
        line += &format!(" {}", i);
    }
    lines.push(line);

    let mut line = String::from(" ");
    for _ in 0..field.height() {
        line += " -";
    }
    lines.push(line);

    for row in 0..field.height() {
        let mut line = format!("{}", row_to_letter(row));
        for column in 0..field.width() {
            line += "|";
            match field.get(Location::new(column, row)).unwrap() {
                Cell::Empty => {
                    if ships
                        .values()
                        .any(|s| s.contains(Location::new(column, row)))
                    {
                        line += "#";
                    } else {
                        line += " ";
                    }
                }
                Cell::Miss => line += "M",
                Cell::Hit => line += "X",
            }
        }
        line += "|";
        lines.push(line);

        let mut line = String::from(" ");
        for _ in 0..field.height() {
            line += " -";
        }
        lines.push(line)
    }
    lines
}

fn ask<T: str::FromStr>(prompt: &str) -> io::Result<T>
where
    <T as str::FromStr>::Err: fmt::Display,
{
    loop {
        let mut stdin = io::BufReader::new(io::stdin());
        print!("{}", prompt);
        io::stdout().flush()?;

        let mut line = String::new();
        stdin.read_line(&mut line)?;
        match line.trim_end_matches('\n').parse() {
            Ok(p) => break Ok(p),
            Err(e) => println!("error: {}", e),
        }
    }
}

fn place_ships<P: Play>(game: &mut P, player_id: PlayerId) -> io::Result<()> {
    for (ship_id, ship) in game.get_player(player_id).unwrap().ships() {
        print_battlefield(game.get_player(player_id).unwrap());

        let ship_name = ship.name();
        loop {
            let location: Location = ask(&format!("Location for {}?: ", ship_name))?;
            let direction: Direction = ask(&format!("Direction for {}?: ", ship_name))?;
            match game.place_ship(player_id, ship_id, location, direction) {
                Ok(_) => break,
                Err(e) => println!("error: {}", e),
            }
        }
    }

    Ok(())
}

fn do_attack<G: Play>(game: &mut G, player1_id: PlayerId, player2_id: PlayerId) -> io::Result<()> {
    loop {
        match game.advance(player1_id, player2_id, ask("guess: ")?) {
            Ok(res) => {
                println!("{}", res);
                break;
            }
            Err(e) => println!("{}", e),
        }
    }
    Ok(())
}

fn print_battlefield(player: &Player) {
    let lines1 = format_battlefield(&HashMap::new(), player.speculative_field());
    let lines2 = format_battlefield(&player.ships(), player.own_field());
    println!("{:22}    {:22}", "Enemy", "Home");
    for (line1, line2) in lines1.iter().zip(lines2.iter()) {
        println!("{:22}    {:22}", line1, line2);
    }
}

fn local_game() -> Result<()> {
    let mut game = Game::new();
    let player1_id = game.add_player("Player 1").unwrap();
    let player2_id = game.add_player("Player 2").unwrap();

    place_ships(&mut game, player1_id)?;

    game.get_player_mut(player2_id)
        .unwrap()
        .place_ships_automatically();

    while game.winner().is_none() {
        print_battlefield(game.get_player(player1_id).unwrap());

        do_attack(&mut game, player1_id, player2_id)?;
        if game.winner().is_some() {
            break;
        }

        println!("{}'s turn", game.get_player(player2_id).unwrap().name());
        println!(
            "{}",
            game.advance_automatically(player1_id, player2_id).unwrap()
        );
    }

    let winner = game.winner().unwrap();
    println!("{} wins!", game.get_player(winner).unwrap().name());
    Ok(())
}

fn server() -> Result<()> {
    simple_logger::init().unwrap();

    let listener = net::TcpListener::bind("0.0.0.0:0")?;
    info!("listening on {}", listener.local_addr()?);

    let mut game_server = BlockingGameServer::new();
    game_server.run(&listener);
    Ok(())
}

fn client(address: &str) -> Result<()> {
    let conn = net::TcpStream::connect(address)?;

    let name: String = ask("name: ")?;
    let mut game = BlockingGameClient::new(conn, &name)?;

    let player_id = game.player_id();
    place_ships(&mut game, player_id)?;

    print_battlefield(game.player().unwrap());

    println!("waiting for other player");
    if let Some(result) = game.wait_for_turn()? {
        println!("{}", result);
    }

    print_battlefield(game.player().unwrap());

    let other_player_id = game.other_player_ids()[0];

    let mut winner = None;
    while winner.is_none() {
        do_attack(&mut game, player_id, other_player_id)?;

        winner = game.winner()?;
        if winner.is_some() {
            break;
        }

        print_battlefield(game.get_player(player_id).unwrap());

        println!("waiting for other player");
        if let Some(result) = game.wait_for_turn()? {
            println!("{}", result);
        }

        print_battlefield(game.get_player(player_id).unwrap());

        winner = game.winner()?;
    }

    if winner.unwrap() == player_id {
        println!("you win");
    } else {
        println!("you lose");
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let mut iter = args.iter().skip(1).map(|s| s.as_ref());

    match iter.next() {
        None => local_game()?,
        Some("server") => server()?,
        Some("client") => client(iter.next().unwrap())?,
        Some(s) => println!("invalid command {}", s),
    }

    Ok(())
}
