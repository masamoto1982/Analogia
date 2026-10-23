// `#[wasm_bindgen]` expands into glue code; nothing hand-written here is unsafe.

//! The browser bindings: the `AjisaiInterpreter` class the playground drives.
//! The shapes returned here are the ones `src/wasm-interpreter-types.ts`
//! declares. Every structured value is built as `serde_json` and handed to
//! the JS engine's own `JSON.parse`, which keeps this file free of
//! per-field `Reflect` calls.

use crate::interpreter::{Interpreter, Snapshot, CORE_WORDS, MAX_CALL_DEPTH, MAX_RUN_STEPS};
use crate::value::Value;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Route Rust panics to the browser console. Called once by the TS loader.
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

fn to_js(value: serde_json::Value) -> JsValue {
    js_sys::JSON::parse(&value.to_string()).expect("serde_json writes valid JSON")
}

#[derive(Deserialize)]
struct UserWordData {
    name: String,
    definition: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// FNV-1a over the definition: enough to tell two definitions apart when the
/// playground deduplicates an import.
fn identity(definition: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in definition.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[wasm_bindgen]
pub struct AjisaiInterpreter {
    interpreter: Interpreter,
}

impl Default for AjisaiInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl AjisaiInterpreter {
    fn user_words_json(&self) -> serde_json::Value {
        self.interpreter
            .words
            .iter()
            .map(|(name, word)| {
                json!({ "name": name, "definition": word.definition(), "description": word.description })
            })
            .collect()
    }

    fn stack_json(&self) -> serde_json::Value {
        self.interpreter.stack.iter().map(Value::protocol).collect()
    }
}

#[wasm_bindgen]
impl AjisaiInterpreter {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        AjisaiInterpreter {
            interpreter: Interpreter::new(),
        }
    }

    /// The resource ceilings this interpreter runs under, as JSON.
    pub fn host_profile(&self) -> String {
        json!({
            "profile": "browser-playground",
            "limits": {
                "maxExecutionSteps": self.interpreter.max_steps(),
                "maxCallDepth": MAX_CALL_DEPTH,
                "maxRunSteps": MAX_RUN_STEPS,
            },
        })
        .to_string()
    }

    /// Run `code`. Answers a Promise so a core that does real async work can
    /// drop in without changing the playground.
    pub fn execute(&mut self, code: &str) -> js_sys::Promise {
        let result = match self.interpreter.execute(code) {
            Ok(()) => json!({
                "status": "OK",
                "output": self.interpreter.take_output(),
                "userWords": self.user_words_json(),
            }),
            Err(e) => {
                let mut obj = json!({
                    "status": "ERROR",
                    "message": e.message,
                    "error": true,
                    "output": self.interpreter.take_output(),
                    "errorInfo": { "category": e.category },
                });
                let changes = self.interpreter.changes_this_run();
                if !changes.is_empty() {
                    obj["discardedDictionaryChanges"] = json!(changes);
                }
                obj
            }
        };
        js_sys::Promise::resolve(&to_js(result))
    }

    pub fn reset(&mut self) -> JsValue {
        self.interpreter.reset();
        to_js(json!({
            "status": "OK",
            "output": "System reinitialized.",
            "stack": self.stack_json(),
            "userWords": self.user_words_json(),
        }))
    }

    pub fn collect_stack(&self) -> JsValue {
        to_js(self.stack_json())
    }

    /// `[name, hasDependents]` for each User word: whether another word or
    /// a pipe still reads it.
    pub fn collect_user_words_info(&self) -> JsValue {
        let info: Vec<_> = self
            .interpreter
            .words
            .keys()
            .map(|name| json!([name, self.interpreter.user_of(name).is_some()]))
            .collect();
        to_js(json!(info))
    }

    pub fn collect_word_identities(&self) -> JsValue {
        let ids: Vec<_> = self
            .interpreter
            .words
            .iter()
            .map(|(name, word)| json!([name, identity(&word.definition())]))
            .collect();
        to_js(json!(ids))
    }

    pub fn collect_core_words_info(&self) -> JsValue {
        let info: Vec<_> = CORE_WORDS
            .iter()
            .map(|(n, s, x)| json!([n, s, x]))
            .collect();
        to_js(json!(info))
    }

    pub fn lookup_word_definition(&self, name: &str) -> Option<String> {
        self.interpreter
            .words
            .get(&name.to_uppercase())
            .map(|w| w.definition())
    }

    pub fn lookup_word_description(&self, name: &str) -> Option<String> {
        self.interpreter
            .words
            .get(&name.to_uppercase())
            .and_then(|w| w.description.clone())
    }

    /// `{ kind, text }` for a name in the dictionary, or `null`.
    pub fn resolve_host_lookup(&self, name: &str) -> JsValue {
        let upper = name.to_uppercase();
        if let Some((_, summary, syntax)) = CORE_WORDS.iter().find(|(n, _, _)| *n == upper) {
            return to_js(
                json!({ "kind": "documentation", "text": format!("{summary}\n\n{syntax}") }),
            );
        }
        match self.interpreter.words.get(&upper) {
            Some(w) => to_js(json!({
                "kind": "definition",
                "text": format!("[ {} ] '{upper}' DEF", w.definition()),
            })),
            None => JsValue::NULL,
        }
    }

    /// The stack and the machine (its tanks, pipes and clock), in a form
    /// `restore_stack_snapshot` reads back exactly. The playground carries
    /// this string into its worker and back, so the machine lives in it too.
    pub fn snapshot_stack(&self) -> String {
        serde_json::to_string(&self.interpreter.snapshot()).expect("values always serialize")
    }

    pub fn restore_stack_snapshot(&mut self, snapshot_json: &str) -> Result<(), JsValue> {
        let snapshot: Snapshot =
            serde_json::from_str(snapshot_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.interpreter.restore(snapshot);
        Ok(())
    }

    pub fn clear_stack(&mut self) {
        self.interpreter.stack.clear();
    }

    /// Restore every word it can; answers `[name, reason]` for the rest.
    pub fn restore_user_words(&mut self, words: JsValue) -> Result<JsValue, JsValue> {
        let text: String = js_sys::JSON::stringify(&words)?.into();
        let words: Vec<UserWordData> =
            serde_json::from_str(&text).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let mut skipped = Vec::new();
        for word in words {
            let Some(definition) = word.definition else {
                continue;
            };
            if let Err(reason) = self
                .interpreter
                .define(&word.name, &definition, word.description)
            {
                skipped.push(json!([word.name, reason]));
            }
        }
        Ok(to_js(json!(skipped)))
    }

    pub fn set_max_execution_steps(&mut self, steps: f64) {
        if steps.is_finite() && steps > 0.0 {
            self.interpreter.set_max_steps(steps as u64);
        }
    }
}
