//! wasm-bindgen wrapper exposing `nullime_engine::Engine` to the web demo.
//! Responses become plain JS objects with the Rust (snake_case) field names.

use nullime_engine::{Key, KeyEvent};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Engine(nullime_engine::Engine, Option<u32>);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine(nullime_engine::Engine::new(), None)
    }

    /// An engine converting Japanese with a `nullime-ja.dict` file's bytes;
    /// throws when they are not a valid asset.
    #[wasm_bindgen(js_name = withDictionary)]
    pub fn with_dictionary(bytes: Vec<u8>) -> Result<Engine, JsError> {
        let asset = nullime_engine::ja_asset::JaAsset::new(bytes)?;
        let entries = asset.entry_count() as u32;
        Ok(Engine(nullime_engine::Engine::with_ja_dictionary(asset), Some(entries)))
    }

    /// Entry count of the loaded dictionary, for the status line; undefined
    /// for the built-in sample.
    #[wasm_bindgen(getter, js_name = dictionaryEntries)]
    pub fn dictionary_entries(&self) -> Option<u32> {
        self.1
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

    /// Score breakdown of the current composition (`Engine::explain`).
    pub fn explain(&self) -> JsValue {
        to_js(&self.0.explain())
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

fn to_js(value: &impl Serialize) -> JsValue {
    // json_compatible: `preedit: None` becomes `null` rather than `undefined`.
    // `Response` and `Explanation` hold only strings, numbers, bools, options
    // and lists, so serialization cannot fail.
    value.serialize(&serde_wasm_bindgen::Serializer::json_compatible()).expect("value serializes")
}
