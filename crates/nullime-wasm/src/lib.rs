//! wasm-bindgen wrapper exposing `nullime_engine::Engine` to the web demo.
//! Responses become plain JS objects with the Rust (snake_case) field names.

use nullime_engine::{Key, KeyEvent, Response};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Engine(nullime_engine::Engine);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine(nullime_engine::Engine::new())
    }

    /// `key`: 0=Other 1=Char 2=Space 3=Enter 4=Backspace 5=Left 6=Right 7=Up 8=Down.
    /// `ch`: the printable ASCII byte when `key == 1`, otherwise ignored.
    pub fn key(&mut self, key: u32, ch: u32, shift: bool, modifier: bool) -> JsValue {
        let key = match key {
            1 => match u8::try_from(ch) {
                Ok(c) => Key::Char(c),
                Err(_) => Key::Other,
            },
            2 => Key::Space,
            3 => Key::Enter,
            4 => Key::Backspace,
            5 => Key::Left,
            6 => Key::Right,
            7 => Key::Up,
            8 => Key::Down,
            _ => Key::Other,
        };
        to_js(&self.0.key(KeyEvent { key, shift, modifier }))
    }

    pub fn reset(&mut self) -> JsValue {
        to_js(&self.0.reset())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn to_js(response: &Response) -> JsValue {
    // json_compatible: `preedit: None` becomes `null` rather than `undefined`.
    // `Response` holds only strings, numbers, bools and options, so
    // serialization cannot fail.
    response
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .expect("Response serializes")
}
