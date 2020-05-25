// copyright 2020 Remi Bernotavicius
use battleship_game::client::{ClientResponse, GameClient};
use battleship_game::protocol::Response;
use battleship_game::{row_to_letter, BattleField, Cell, Direction, Location, Ship, ShipId};
use serde::Deserialize as _;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, WebSocket};

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no global `window` exists")
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("should register `requestAnimationFrame` OK");
}

enum GameState {
    Connecting,
    PlacingShip(ShipId, Direction, WebSocket),
    WaitingForPlayerAdd(WebSocket),
    WaitingForTurn(WebSocket),
    WaitingForAttackResult(WebSocket),
    MyTurn(WebSocket),
    Error,
}

impl GameState {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Error)
    }
}

struct RenderableField {
    x: f64,
    y: f64,
    width: usize,
    height: usize,
}

impl RenderableField {
    const HEADER_SIZE: f64 = 15.0;
    const CELL_SIZE: f64 = 50.0;

    fn location(&self, x: u32, y: u32) -> Option<Location> {
        let grid_x = self.x + Self::HEADER_SIZE;
        let grid_y = self.y + Self::HEADER_SIZE;

        if (x as f64) < grid_x || (y as f64) < grid_y {
            return None;
        }

        let location = Location::new(
            (((x as f64) - grid_x) / Self::CELL_SIZE) as usize,
            (((y as f64) - grid_y) / Self::CELL_SIZE) as usize,
        );

        if location.column > self.width || location.row > self.height {
            None
        } else {
            Some(location)
        }
    }

    fn render(
        &self,
        drawing_context: &mut web_sys::CanvasRenderingContext2d,
        ships: &HashMap<ShipId, Ship>,
        field: &BattleField,
        mouse_location: Option<Location>,
    ) {
        drawing_context.set_fill_style(&JsValue::from_str("black"));
        drawing_context.set_font("10px arial");

        let grid_x = self.x + Self::HEADER_SIZE;
        let grid_y = self.y + Self::HEADER_SIZE;

        for i in 0..self.height {
            drawing_context.move_to(grid_x, grid_y + (i as f64) * Self::CELL_SIZE);
            drawing_context.line_to(
                grid_x + (self.width as f64) * Self::CELL_SIZE,
                grid_y + (i as f64) * Self::CELL_SIZE,
            );
            drawing_context
                .fill_text(
                    &(i + 1).to_string(),
                    self.x,
                    self.y + (Self::CELL_SIZE * 3.0 / 4.0) + (i as f64) * Self::CELL_SIZE,
                )
                .unwrap();
        }
        drawing_context.move_to(grid_x, grid_y + (self.height as f64) * Self::CELL_SIZE);
        drawing_context.line_to(
            grid_x + (self.width as f64) * Self::CELL_SIZE,
            grid_y + (self.height as f64) * Self::CELL_SIZE,
        );

        for i in 0..self.width {
            drawing_context.move_to(grid_x + (i as f64) * Self::CELL_SIZE, grid_y);
            drawing_context.line_to(
                grid_x + (i as f64) * Self::CELL_SIZE,
                grid_y + (self.height as f64) * Self::CELL_SIZE,
            );
            drawing_context
                .fill_text(
                    &row_to_letter(i).to_string(),
                    self.x + (Self::CELL_SIZE * 3.0 / 4.0) + (i as f64) * Self::CELL_SIZE,
                    self.y,
                )
                .unwrap();
        }
        drawing_context.move_to(grid_x + (self.width as f64) * Self::CELL_SIZE, grid_y);
        drawing_context.line_to(
            grid_x + (self.width as f64) * Self::CELL_SIZE,
            grid_y + (self.height as f64) * Self::CELL_SIZE,
        );

        for row in 0..self.height {
            for column in 0..self.width {
                drawing_context.set_fill_style(&JsValue::from_str("white"));
                if mouse_location == Some(Location::new(column, row)) {
                    drawing_context.set_fill_style(&JsValue::from_str("#ded9d9"));
                }
                if ships
                    .values()
                    .any(|s| s.contains(Location::new(column, row)))
                {
                    drawing_context.set_fill_style(&JsValue::from_str("#799394"));
                }

                match field.get(Location::new(column, row)).unwrap() {
                    Cell::Miss => {
                        drawing_context.set_fill_style(&JsValue::from_str("#1ce5ed"));
                    }
                    Cell::Hit => {
                        drawing_context.set_fill_style(&JsValue::from_str("#ff6600"));
                    }
                    _ => (),
                }
                drawing_context.fill_rect(
                    grid_x + (column as f64) * Self::CELL_SIZE,
                    grid_y + (row as f64) * Self::CELL_SIZE,
                    Self::CELL_SIZE,
                    Self::CELL_SIZE,
                );
            }
        }
    }
}

struct GameFields {
    own_field: RenderableField,
    speculative_field: RenderableField,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageLevel {
    Info,
    Warn,
    Error,
}

impl MessageLevel {
    fn color(&self) -> &'static str {
        match self {
            MessageLevel::Info => "blue",
            MessageLevel::Warn => "orange",
            MessageLevel::Error => "red",
        }
    }
}

struct Game {
    client: GameClient,
    drawing_context: web_sys::CanvasRenderingContext2d,
    mouse_pos: Option<(u32, u32)>,
    canvas_width: u32,
    canvas_height: u32,
    message: Option<(String, MessageLevel)>,
    state: GameState,
    fields: Option<GameFields>,
}

impl Game {
    fn new(
        drawing_context: web_sys::CanvasRenderingContext2d,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        Self {
            client: GameClient::new(),
            drawing_context,
            mouse_pos: None,
            canvas_width,
            canvas_height,
            message: None,
            state: GameState::Connecting,
            fields: None,
        }
    }

    fn message<S: Into<String>>(&mut self, message: S, level: MessageLevel) {
        let message = message.into();
        console_log!("{}", &message);
        self.message = Some((message, level));
    }

    fn handle_response(&mut self, response: ClientResponse) {
        match response {
            ClientResponse::Attack(result) => {
                use MessageLevel::{Info, Warn};
                let color = if result.is_hit() { Warn } else { Info };
                match self.state.take() {
                    GameState::WaitingForTurn(socket) => {
                        self.message(format!("Enemy attack: {}, your turn", &result), color);
                        self.state = GameState::MyTurn(socket);
                    }
                    GameState::WaitingForAttackResult(socket) => {
                        self.message(format!("Your attack: {}, waiting for enemy", result), color);
                        self.wait_for_turn(socket);
                    }
                    _ => (),
                }
            }
            ClientResponse::None => match self.state.take() {
                GameState::WaitingForPlayerAdd(socket) | GameState::PlacingShip(_, _, socket) => {
                    let player = self.client.player().unwrap();
                    self.fields = Some(GameFields {
                        own_field: RenderableField {
                            x: 10.0,
                            y: 55.0,
                            width: player.own_field().width(),
                            height: player.own_field().height(),
                        },
                        speculative_field: RenderableField {
                            x: 550.0,
                            y: 55.0,
                            width: player.speculative_field().width(),
                            height: player.speculative_field().height(),
                        },
                    });
                    self.try_to_place_ship(socket)
                }
                GameState::WaitingForTurn(socket) => {
                    self.message("Your turn", MessageLevel::Info);
                    self.state = GameState::MyTurn(socket);
                }
                _ => (),
            },
            _ => (),
        }
    }

    fn try_to_place_ship(&mut self, socket: WebSocket) {
        // Choose an unplaced ship
        let ships = self.client.player().unwrap().ships();
        let ship = ships.into_iter().filter(|(_, v)| !v.placed()).next();
        if let Some((ship_id, ship)) = ship {
            self.message(format!("Place {}", ship.name()), MessageLevel::Info);
            self.state = GameState::PlacingShip(ship_id, Direction::South, socket);
        } else {
            self.message("Waiting for turn", MessageLevel::Info);
            self.wait_for_turn(socket)
        }
    }

    fn wait_for_turn(&mut self, socket: WebSocket) {
        let request = self.client.wait_for_turn();
        let message = serde_json::to_string(&request).unwrap();
        socket.send_with_str(&message).unwrap();
        console_log!("{:?}", request);
        self.state = GameState::WaitingForTurn(socket);
    }

    fn on_data<R: io::Read>(&mut self, reader: &mut R) -> bool {
        if let Ok(response) =
            Response::deserialize(&mut serde_json::Deserializer::from_reader(reader))
        {
            console_log!("{:?}", response);
            match self.client.handle_response(response) {
                Ok(res) => self.handle_response(res),
                Err(e) => {
                    self.message(e.to_string(), MessageLevel::Error);
                }
            }
            true
        } else {
            false
        }
    }

    fn render(&mut self) {
        self.drawing_context.clear_rect(
            0.0,
            0.0,
            self.canvas_width as f64,
            self.canvas_height as f64,
        );
        self.drawing_context.begin_path();

        if let Some((message, level)) = &self.message {
            self.drawing_context
                .set_fill_style(&JsValue::from_str(level.color()));
            self.drawing_context.fill_rect(10.0, 5.0, 1224.0, 35.0);
            self.drawing_context.set_font("25px arial");
            self.drawing_context
                .set_fill_style(&JsValue::from_str("white"));
            self.drawing_context
                .fill_text(message.as_str(), 20.0, 30.0)
                .unwrap();
        }

        if let Some(fields) = &self.fields {
            let location = self.mouse_location(&fields.own_field);
            let player = self.client.player().unwrap();

            let ships = &player.ships();
            fields.own_field.render(
                &mut self.drawing_context,
                ships,
                player.own_field(),
                location,
            );

            let location = self.mouse_location(&fields.speculative_field);
            let player = self.client.player().unwrap();
            fields.speculative_field.render(
                &mut self.drawing_context,
                &HashMap::new(),
                player.speculative_field(),
                location,
            );
        }

        self.drawing_context.stroke();
    }

    fn mouse_location(&self, field: &RenderableField) -> Option<Location> {
        if let &Some((x, y)) = &self.mouse_pos {
            field.location(x, y)
        } else {
            None
        }
    }

    fn on_connect(&mut self, socket: &WebSocket) {
        console_log!("established connection to server");

        let request = self.client.add_player("remi");
        let message = serde_json::to_string(&request).unwrap();
        socket.send_with_str(&message).unwrap();
        console_log!("{:?}", request);

        self.state = GameState::WaitingForPlayerAdd(socket.clone());
    }

    fn on_mouse_move(&mut self, x: u32, y: u32) {
        self.mouse_pos = Some((x, y));
    }

    fn on_mouse_click(&mut self, x: u32, y: u32) {
        match self.state.take() {
            GameState::MyTurn(socket) => {
                let field = &self.fields.as_ref().unwrap().speculative_field;
                if let Some(location) = field.location(x, y) {
                    let player_id = self.client.player_id();
                    let other_player_id = self.client.other_player_ids()[0];

                    let request = self.client.advance(player_id, other_player_id, location);

                    let message = serde_json::to_string(&request).unwrap();
                    socket.send_with_str(&message).unwrap();
                    console_log!("{:?}", request);

                    self.state = GameState::WaitingForAttackResult(socket);
                }
            }
            GameState::PlacingShip(ship_id, direction, socket) => {
                let field = &self.fields.as_ref().unwrap().own_field;
                if let Some(location) = field.location(x, y) {
                    let player_id = self.client.player_id();
                    let request = self
                        .client
                        .place_ship(player_id, ship_id, location, direction);
                    let message = serde_json::to_string(&request).unwrap();
                    socket.send_with_str(&message).unwrap();
                    console_log!("{:?}", request);

                    self.try_to_place_ship(socket);
                }
            }
            s => {
                self.state = s;
            }
        }
    }
}

fn connect_websocket(game: Rc<RefCell<Game>>) -> Result<(), JsValue> {
    // connect to the server
    let ws = WebSocket::new("ws://10.11.12.38:9090")?;
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // when we get a message, forward it to the game
    let mut buffer = vec![];
    let cloned_game = game.clone();
    let onmessage_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
        if let Ok(abuf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
            let array = js_sys::Uint8Array::new(&abuf);
            buffer.extend(&array.to_vec());
            let mut game = cloned_game.borrow_mut();

            let mut reader = buffer.as_slice();
            if game.on_data(&mut reader) {
                buffer = reader.to_vec();
            }
        } else {
            let mut game = cloned_game.borrow_mut();
            game.message(
                format!("Connection error, received unknown data: {:?}", e.data()),
                MessageLevel::Error,
            );
        }
    }) as Box<dyn FnMut(MessageEvent)>);
    ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
    onmessage_callback.forget();

    // when we get an error, let's display a message
    let cloned_game = game.clone();
    let onerror_callback = Closure::wrap(Box::new(move |e: ErrorEvent| {
        let mut game = cloned_game.borrow_mut();
        game.message(format!("error: {:?}", e), MessageLevel::Error);
    }) as Box<dyn FnMut(ErrorEvent)>);
    ws.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
    onerror_callback.forget();

    // when we finish connecting, call into the game
    let cloned_ws = ws.clone();
    let cloned_game = game.clone();
    let onopen_callback = Closure::wrap(Box::new(move |_| {
        let mut game = cloned_game.borrow_mut();
        game.on_connect(&cloned_ws);
    }) as Box<dyn FnMut(JsValue)>);
    ws.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
    onopen_callback.forget();

    Ok(())
}

fn canvas() -> web_sys::HtmlCanvasElement {
    let document = window().document().unwrap();
    let canvas = document.get_element_by_id("canvas").unwrap();
    canvas
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap()
}

fn get_drawing_context(canvas: &web_sys::HtmlCanvasElement) -> web_sys::CanvasRenderingContext2d {
    canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .unwrap()
}

fn set_up_rendering(game: Rc<RefCell<Game>>) {
    let f = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        game.borrow_mut().render();

        // Schedule ourself for another requestAnimationFrame callback.
        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut()>));

    request_animation_frame(g.borrow().as_ref().unwrap());
}

fn set_up_input(game: Rc<RefCell<Game>>) {
    let canvas = canvas();

    let cloned_game = game.clone();
    let closure = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        cloned_game
            .borrow_mut()
            .on_mouse_move(event.offset_x() as u32, event.offset_y() as u32);
    }) as Box<dyn FnMut(_)>);

    canvas
        .add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())
        .unwrap();
    closure.forget();

    let cloned_game = game.clone();
    let closure = Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
        cloned_game
            .borrow_mut()
            .on_mouse_click(event.offset_x() as u32, event.offset_y() as u32);
    }) as Box<dyn FnMut(_)>);

    canvas
        .add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())
        .unwrap();
    closure.forget();
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let canvas = canvas();
    let canvas_width = 1224;
    let canvas_height = 590;
    canvas.set_width(canvas_width);
    canvas.set_height(canvas_height);

    let drawing_context = get_drawing_context(&canvas);
    let game = Rc::new(RefCell::new(Game::new(
        drawing_context,
        canvas_width,
        canvas_height,
    )));

    connect_websocket(game.clone())?;
    set_up_rendering(game.clone());
    set_up_input(game.clone());

    Ok(())
}
