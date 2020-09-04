// Copyright 2020 Remi Bernotavicius

use matches::matches;
use rand::{self, rngs::ThreadRng, Rng};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::{fmt, ops, result, str};

pub mod client;
pub mod protocol;
pub mod server;

const MAX_PLAYERS: usize = 2;

#[derive(
    Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash,
)]
pub struct GameId(usize);

impl GameId {
    fn incr(&self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

impl str::FromStr for GameId {
    type Err = <usize as str::FromStr>::Err;
    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        Ok(Self(s.parse()?))
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
pub struct PlayerId(GameId, usize);

impl str::FromStr for PlayerId {
    type Err = String;
    fn from_str(s: &str) -> result::Result<Self, Self::Err> {
        let mut s = s.split(".");
        match (s.next(), s.next()) {
            (Some(s1), Some(s2)) => {
                let game_id = s1.parse::<GameId>().map_err(|e| e.to_string())?;
                let id = s2.parse::<usize>().map_err(|e| e.to_string())?;
                Ok(Self(game_id, id))
            }
            _ => Err("expected <number>.<number>".into()),
        }
    }
}

impl fmt::Display for PlayerId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}.{}", self.0, self.1)
    }
}

impl PlayerId {
    pub fn game_id(&self) -> GameId {
        self.0
    }

    fn incr(&self) -> Self {
        Self(self.0, self.1.wrapping_add(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    InvalidLocation(Location),
    InvalidShipLocation(Location, Direction),
    InvalidSelfAttack,
    UnknownShipId(ShipId),
    ShipPlacementConflict(String),
    UnknownPlayer(PlayerId),
    UnknownGame(GameId),
    NotYourTurn(String),
    TooManyPlayers,
    CommunicationError,
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidLocation(loc) => write!(fmt, "invalid location {}", loc),
            Self::InvalidShipLocation(loc, dir) => {
                write!(fmt, "location {}, {} places ship off map", loc, dir)
            }
            Self::UnknownShipId(ship_id) => write!(fmt, "unknown ship id {:?}", ship_id),
            Self::ShipPlacementConflict(name) => {
                write!(fmt, "unable to place ship, conflict with {}", name)
            }
            Self::UnknownPlayer(_) => write!(fmt, "unknown player"),
            Self::UnknownGame(_) => write!(fmt, "unknown game"),
            Self::NotYourTurn(player) => write!(fmt, "it is not {}'s turn", player),
            Self::TooManyPlayers => write!(fmt, "too many players"),
            Self::InvalidSelfAttack => write!(fmt, "cannot attack yourself"),
            Self::CommunicationError => write!(fmt, "communication error"),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Game {
    id: GameId,
    players: HashMap<PlayerId, Player>,
    current_turn: Option<PlayerId>,
}

impl Game {
    pub fn new(id: GameId) -> Self {
        Self {
            id,
            players: HashMap::new(),
            current_turn: None,
        }
    }

    pub fn add_player(&mut self, name: &str) -> Result<PlayerId> {
        if self.players.len() >= MAX_PLAYERS {
            return Err(Error::TooManyPlayers);
        }

        let max_id = self
            .players
            .keys()
            .cloned()
            .max()
            .unwrap_or(PlayerId(self.id, 0));
        let id = max_id.incr();
        self.give_player(id, Player::new(name));
        self.current_turn = Some(id);
        Ok(id)
    }

    pub fn get_player_mut(&mut self, player_id: PlayerId) -> Result<&mut Player> {
        self.players
            .get_mut(&player_id)
            .ok_or(Error::UnknownPlayer(player_id))
    }

    fn take_player(&mut self, player_id: PlayerId) -> Result<Player> {
        self.players
            .remove(&player_id)
            .ok_or(Error::UnknownPlayer(player_id))
    }

    fn give_player(&mut self, player_id: PlayerId, player: Player) {
        let res = self.players.insert(player_id, player);
        assert!(res.is_none());
    }

    fn next_turn(&mut self) {
        let current = self.current_turn.unwrap();
        let mut iter = self.players.keys().cycle().skip_while(|&&k| k != current);
        let next = iter.next();
        assert_eq!(next, Some(&current));
        self.current_turn = Some(*iter.next().unwrap());
    }

    pub fn current_turn(&self) -> Option<PlayerId> {
        if self.players.len() == MAX_PLAYERS && self.players.values().all(|p| p.ships_placed()) {
            self.current_turn.clone()
        } else {
            None
        }
    }

    pub fn winner(&self) -> Option<PlayerId> {
        let alive: Vec<_> = self.players.iter().filter(|(_, p)| !p.dead()).collect();
        if alive.len() == 1 {
            Some(*alive[0].0)
        } else {
            None
        }
    }

    pub fn get_players(&self) -> Vec<PlayerId> {
        self.players.keys().cloned().collect()
    }
}

pub trait Play {
    fn advance(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
        guess: Location,
    ) -> Result<AttackResult>;

    fn advance_automatically(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
    ) -> Result<AttackResult>;

    fn place_ship(
        &mut self,
        player_id: PlayerId,
        ship: ShipId,
        location: Location,
        direction: Direction,
    ) -> Result<()>;

    fn get_player(&self, player_id: PlayerId) -> Result<&Player>;
}

impl Play for Game {
    fn advance(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
        guess: Location,
    ) -> Result<AttackResult> {
        if Some(player_a_id) != self.current_turn {
            return Err(Error::NotYourTurn(
                self.get_player(player_a_id)?.name().into(),
            ));
        }
        if player_a_id == player_b_id {
            return Err(Error::InvalidSelfAttack);
        }

        let mut player_a = self.take_player(player_a_id)?;
        let mut player_b = self.take_player(player_b_id)?;
        let res = player_a.attack(&mut player_b, guess);
        self.give_player(player_a_id, player_a);
        self.give_player(player_b_id, player_b);
        if res.is_ok() {
            self.next_turn();
        }
        res
    }

    fn advance_automatically(
        &mut self,
        player_a_id: PlayerId,
        player_b_id: PlayerId,
    ) -> Result<AttackResult> {
        if Some(player_a_id) != self.current_turn {
            return Err(Error::NotYourTurn(
                self.get_player(player_a_id)?.name().into(),
            ));
        }

        let mut player_a = self.take_player(player_a_id)?;
        let mut player_b = self.take_player(player_b_id)?;
        let res = player_a.attack_automatically(&mut player_b);
        self.give_player(player_a_id, player_a);
        self.give_player(player_b_id, player_b);
        self.next_turn();
        Ok(res)
    }

    fn place_ship(
        &mut self,
        player_id: PlayerId,
        ship: ShipId,
        location: Location,
        direction: Direction,
    ) -> Result<()> {
        let player = self.get_player_mut(player_id)?;
        player.place_ship(ship, location, direction)
    }

    fn get_player(&self, player_id: PlayerId) -> Result<&Player> {
        self.players
            .get(&player_id)
            .ok_or(Error::UnknownPlayer(player_id))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Player {
    own_field: BattleField,
    speculative_field: BattleField,
    ships: HashMap<ShipId, Ship>,
    name: String,
}

impl Player {
    fn new<S: Into<String>>(name: S) -> Self {
        let mut ships = HashMap::new();
        ships.insert(ShipId(1), Ship::new(ShipKind::Carrier));
        ships.insert(ShipId(2), Ship::new(ShipKind::Battleship));
        ships.insert(ShipId(3), Ship::new(ShipKind::Destroyer));
        ships.insert(ShipId(4), Ship::new(ShipKind::Submarine));
        ships.insert(ShipId(5), Ship::new(ShipKind::PatrolBoat));

        Self {
            own_field: BattleField::default(),
            speculative_field: BattleField::default(),
            ships,
            name: name.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ships(&self) -> HashMap<ShipId, Ship> {
        self.ships.clone()
    }

    pub fn own_field(&self) -> &BattleField {
        &self.own_field
    }

    pub fn speculative_field(&self) -> &BattleField {
        &self.speculative_field
    }

    fn get_ship(&mut self, ship_id: ShipId) -> Result<&Ship> {
        self.ships
            .get(&ship_id)
            .ok_or(Error::UnknownShipId(ship_id))
    }

    pub fn place_ship(
        &mut self,
        ship_id: ShipId,
        location: Location,
        direction: Direction,
    ) -> Result<()> {
        let ship_size = self.get_ship(ship_id)?.size();

        // Make sure the placement doesn't conflict with any other existing ship.
        let other_ships = self.ships.iter().filter(|(&id, _)| id != ship_id);
        for (_, other_ship) in other_ships {
            for s in 0..ship_size {
                let loc = location + Vector::new(direction, s);
                if matches!(loc, Some(l) if other_ship.contains(l)) {
                    return Err(Error::ShipPlacementConflict(other_ship.name()));
                }
            }
        }

        self.ships
            .get_mut(&ship_id)
            .ok_or(Error::UnknownShipId(ship_id))?
            .place(&self.own_field, location, direction)?;

        Ok(())
    }

    pub fn place_ships_automatically(&mut self) {
        let mut rng = rand::thread_rng();
        for ship in self.ships.values_mut() {
            loop {
                let location = Location::random(&mut rng, &self.own_field);
                let direction = Direction::random(&mut rng);
                if ship.place(&self.own_field, location, direction).is_ok() {
                    break;
                }
            }
        }
    }

    pub fn ships_placed(&self) -> bool {
        self.ships.values().all(|s| s.placed())
    }

    pub fn attack(
        &mut self,
        other_player: &mut Player,
        location: Location,
    ) -> Result<AttackResult> {
        if !matches!(other_player.own_field.get(location)?, Cell::Empty) {
            return Err(Error::InvalidLocation(location));
        }

        let mut result = AttackResult::Miss;
        for ship in other_player.ships.values_mut() {
            result = ship.attack(location);
            if result.is_hit() {
                break;
            }
        }

        if result.is_hit() {
            other_player.own_field.record_hit(location)?;
            self.speculative_field.record_hit(location)?;
        } else {
            other_player.own_field.record_miss(location)?;
            self.speculative_field.record_miss(location)?;
        }
        Ok(result)
    }

    pub fn attack_automatically(&mut self, other_player: &mut Player) -> AttackResult {
        let hits: Vec<Location> = self
            .speculative_field
            .iter()
            .filter_map(|(l, c)| if c == Cell::Hit { Some(l) } else { None })
            .collect();

        fn neighbors(location: Location) -> HashSet<Location> {
            let mut n = HashSet::new();
            n.insert(Location::new(
                location.column.saturating_add(1),
                location.row,
            ));
            n.insert(Location::new(
                location.column.saturating_sub(1),
                location.row,
            ));
            n.insert(Location::new(
                location.column,
                location.row.saturating_add(1),
            ));
            n.insert(Location::new(
                location.column,
                location.row.saturating_sub(1),
            ));
            n
        }

        for hit in hits {
            for neigh in neighbors(hit) {
                if let Ok(res) = self.attack(other_player, neigh) {
                    return res;
                }
            }
        }

        let mut rng = rand::thread_rng();
        loop {
            if let Ok(res) = self.attack(
                other_player,
                Location::random(&mut rng, &other_player.own_field),
            ) {
                break res;
            }
        }
    }

    fn dead(&self) -> bool {
        self.ships.values().all(|s| s.sunk())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ShipId(usize);

#[derive(Clone, Debug, Serialize, Deserialize)]
enum ShipState {
    Healthy,
    Hit(usize),
    Sunk,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum AttackResult {
    Hit,
    Miss,
    Sunk(String),
}

impl AttackResult {
    pub fn is_hit(&self) -> bool {
        match self {
            Self::Hit => true,
            Self::Sunk(_) => true,
            Self::Miss => false,
        }
    }
}

impl fmt::Display for AttackResult {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Hit => write!(fmt, "a hit"),
            Self::Miss => write!(fmt, "a miss"),
            Self::Sunk(name) => write!(fmt, "{} was sunk", name),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ship {
    kind: ShipKind,
    state: ShipState,
    location: Option<(Location, Direction)>,
}

impl Ship {
    fn new(kind: ShipKind) -> Self {
        Self {
            kind,
            state: ShipState::Healthy,
            location: None,
        }
    }

    fn sunk(&self) -> bool {
        matches!(self.state, ShipState::Sunk)
    }

    pub fn name(&self) -> String {
        self.kind.to_string()
    }

    pub fn contains(&self, location_in: Location) -> bool {
        if let Some((location, direction)) = self.location.clone() {
            let head = location;
            for s in 0..self.size() {
                let tail = (head + Vector::new(direction, s)).unwrap();
                if location_in == tail {
                    return true;
                }
            }
            return false;
        } else {
            return false;
        }
    }

    fn attack(&mut self, location: Location) -> AttackResult {
        if self.contains(location) {
            let hits = match self.state {
                ShipState::Hit(v) => v + 1,
                _ => 1,
            };
            if hits >= self.size() {
                self.state = ShipState::Sunk;
                AttackResult::Sunk(self.name())
            } else {
                self.state = ShipState::Hit(hits);
                AttackResult::Hit
            }
        } else {
            AttackResult::Miss
        }
    }

    fn size(&self) -> usize {
        self.kind.size()
    }

    fn place(
        &mut self,
        field: &BattleField,
        location: Location,
        direction: Direction,
    ) -> Result<()> {
        let head = location;
        field.require_valid_location(head)?;

        let tail = (head + Vector::new(direction, self.size() - 1))
            .ok_or(Error::InvalidShipLocation(location, direction))?;
        field
            .require_valid_location(tail)
            .map_err(|_| Error::InvalidShipLocation(location, direction))?;

        assert!(self.location.is_none(), "Cannot re-place ship");
        self.location = Some((location, direction));
        Ok(())
    }

    pub fn placed(&self) -> bool {
        self.location.is_some()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum ShipKind {
    Carrier,
    Battleship,
    Destroyer,
    Submarine,
    PatrolBoat,
}

impl fmt::Display for ShipKind {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl ShipKind {
    fn size(&self) -> usize {
        match self {
            ShipKind::Carrier => 5,
            ShipKind::Battleship => 4,
            ShipKind::Destroyer => 3,
            ShipKind::Submarine => 3,
            ShipKind::PatrolBoat => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Cell {
    Empty,
    Miss,
    Hit,
}

pub struct Vector {
    direction: Direction,
    magnitude: usize,
}

impl Vector {
    pub fn new(direction: Direction, magnitude: usize) -> Self {
        Self {
            direction,
            magnitude,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
pub struct Location {
    pub column: usize,
    pub row: usize,
}

impl ops::Add<Vector> for Location {
    type Output = Option<Self>;

    fn add(self, rhs: Vector) -> Option<Self> {
        match rhs.direction {
            Direction::North => self
                .row
                .checked_sub(rhs.magnitude)
                .map(|r| Self::new(self.column, r)),
            Direction::South => self
                .row
                .checked_add(rhs.magnitude)
                .map(|r| Self::new(self.column, r)),
            Direction::East => self
                .column
                .checked_add(rhs.magnitude)
                .map(|c| Self::new(c, self.row)),
            Direction::West => self
                .column
                .checked_sub(rhs.magnitude)
                .map(|c| Self::new(c, self.row)),
        }
    }
}

impl str::FromStr for Location {
    type Err = String;
    fn from_str(s: &str) -> result::Result<Self, String> {
        let mut parts = s.split(',');
        let row = parts.next().ok_or("unexpected end of input".to_string())?;
        let row = row.trim().to_uppercase();
        let row = A_TO_Z.find(&row).ok_or("invalid row; should be (A-Z)")?;

        let column = parts.next().ok_or("unexpected end of input".to_string())?;
        let column = column
            .trim()
            .parse::<usize>()
            .map_err(|e| format!("invalid column; {}", e))?;
        if column == 0 {
            return Err("invalid column".into());
        }

        Ok(Self::new(column - 1, row))
    }
}

#[test]
fn test_location_from_str() {
    assert_eq!("A, 1".parse::<Location>().unwrap(), Location::new(0, 0));
    assert_eq!("B, 2".parse::<Location>().unwrap(), Location::new(1, 1));
    assert_eq!(
        "12, 9".parse::<Location>(),
        Err("invalid row; should be (A-Z)".into())
    );
    assert_eq!(
        "B, C".parse::<Location>(),
        Err("invalid column; invalid digit found in string".into())
    );
}

impl Location {
    pub fn new(column: usize, row: usize) -> Self {
        Self { column, row }
    }

    fn random(rng: &mut ThreadRng, field: &BattleField) -> Self {
        Self::new(
            rng.gen::<usize>() % field.width(),
            rng.gen::<usize>() % field.height(),
        )
    }
}

const A_TO_Z: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn row_to_letter(row: usize) -> char {
    A_TO_Z.chars().nth(row % A_TO_Z.len()).unwrap()
}

#[test]
fn test_row_to_letter() {
    assert_eq!(row_to_letter(0), 'A');
    assert_eq!(row_to_letter(1), 'B');
    assert_eq!(row_to_letter(25), 'Z');
    assert_eq!(row_to_letter(26), 'A');
}

impl fmt::Display for Location {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}, {}", row_to_letter(self.row), self.column + 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    fn random(rng: &mut ThreadRng) -> Self {
        match rng.gen::<usize>() % 4 {
            0 => Direction::North,
            1 => Direction::South,
            2 => Direction::East,
            3 => Direction::West,
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl str::FromStr for Direction {
    type Err = String;
    fn from_str(s: &str) -> result::Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "north" => Ok(Self::North),
            "south" => Ok(Self::South),
            "east" => Ok(Self::East),
            "west" => Ok(Self::West),
            d => Err(format!("invalid direction: {}", d)),
        }
    }
}

#[test]
fn test_direction_from_str() {
    assert_eq!("north".parse::<Direction>().unwrap(), Direction::North);
    assert_eq!("south".parse::<Direction>().unwrap(), Direction::South);
    assert_eq!("east".parse::<Direction>().unwrap(), Direction::East);
    assert_eq!("west".parse::<Direction>().unwrap(), Direction::West);
    assert_eq!(
        "bad".parse::<Direction>(),
        Err("invalid direction: bad".to_string())
    );
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BattleField {
    width: usize,
    height: usize,
    field: Vec<Cell>,
}

impl BattleField {
    fn new(width: usize, height: usize) -> Self {
        let mut field = Vec::new();
        field.resize(width * height, Cell::Empty);
        Self {
            width,
            height,
            field,
        }
    }

    fn iter<'a>(&'a self) -> impl Iterator<Item = (Location, Cell)> + 'a {
        let width = self.width;
        let height = self.height;
        self.field
            .iter()
            .enumerate()
            .map(move |(i, &c)| (Location::new(i % width, i / height), c))
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn get(&self, location: Location) -> Result<Cell> {
        self.require_valid_location(location)?;
        Ok(self.field[location.row * self.width + location.column])
    }

    fn require_valid_location(&self, location: Location) -> Result<()> {
        if location.column >= self.width || location.row >= self.height {
            Err(Error::InvalidLocation(location))
        } else {
            Ok(())
        }
    }

    fn record_hit(&mut self, location: Location) -> Result<()> {
        self.require_valid_location(location)?;
        assert_eq!(self.get(location)?, Cell::Empty);
        self.field[location.row * self.width + location.column] = Cell::Hit;
        Ok(())
    }

    fn record_miss(&mut self, location: Location) -> Result<()> {
        self.require_valid_location(location)?;
        assert_eq!(self.get(location)?, Cell::Empty);
        self.field[location.row * self.width + location.column] = Cell::Miss;
        Ok(())
    }
}

impl Default for BattleField {
    fn default() -> Self {
        Self::new(10, 10)
    }
}
