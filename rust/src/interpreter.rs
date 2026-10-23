//! Analogia: a stack language for wiring up an analog machine.
//!
//! Tanks hold a quantity, pipes move it between tanks at a rate a block
//! computes, and `RUN` lets the machine flow in continuous time (fourth-order
//! Runge–Kutta), the way MONIAC let water find its levels.
//!
//! ```text
//! 100 'BATH' TANK                      # a tank holding 100
//! [ 5 ] 'OUTSIDE' 'BATH' 'TAP' PIPE    # 5 per unit time runs in
//! [ BATH 0.1 MUL ] 'BATH' 'OUTSIDE' 'DRAIN' PIPE
//! 30 0.1 RUN                           # flow for 30 time units
//! ```
//!
//! Inside a rate block a tank's name reads its level, a pipe's name reads its
//! current flow, and `TIME` reads the clock. `[ ... ]` builds a Vector without
//! running anything inside it: words become symbols. `DEF` turns such a
//! Vector into a User word.

use crate::model::{sparkline, Model, Pipe, Tank, OUTSIDE};
use crate::tokenizer::{tokenize, Token};
use crate::value::{format_number, parse_number, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_MAX_STEPS: u64 = 10_000_000;
/// How deeply User words may call each other before the run is refused,
/// so runaway recursion fails cleanly instead of overflowing the native stack.
pub const MAX_CALL_DEPTH: usize = 128;
/// The most integration steps one `RUN` may take.
pub const MAX_RUN_STEPS: usize = 1_000_000;

/// A failed run. `category` is the machine-readable name a host branches on;
/// `message` is display text.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecError {
    pub category: &'static str,
    pub message: String,
}

impl ExecError {
    fn new(category: &'static str, message: impl Into<String>) -> Self {
        ExecError {
            category,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn err(message: impl Into<String>) -> ExecError {
    ExecError::new("runtimeError", message)
}

fn mentions(values: &[Value], name: &str) -> bool {
    values.iter().any(|v| match v {
        Value::Symbol(s) => s == name,
        Value::Vector(items) => mentions(items, name),
        _ => false,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserWord {
    pub body: Vec<Value>,
    pub description: Option<String>,
}

impl UserWord {
    /// The body as source, without the surrounding brackets.
    pub fn definition(&self) -> String {
        self.body
            .iter()
            .map(Value::display)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `(name, summary, syntax)` for the dictionary panel.
pub const CORE_WORDS: &[(&str, &str, &str)] = &[
    ("ADD", "ADD — add two numbers or vectors", "1 2 ADD"),
    ("SUB", "SUB — subtract the top from the second", "5 2 SUB"),
    ("MUL", "MUL — multiply two numbers or vectors", "3 4 MUL"),
    ("DIV", "DIV — divide the second by the top", "1 4 DIV"),
    ("MIN", "MIN — the smaller of two numbers", "3 5 MIN"),
    ("MAX", "MAX — the larger of two numbers", "3 5 MAX"),
    ("EQ", "EQ — whether two values are equal", "1 1 EQ"),
    (
        "LT",
        "LT — whether the second is less than the top",
        "1 2 LT",
    ),
    (
        "GT",
        "GT — whether the second is greater than the top",
        "2 1 GT",
    ),
    (
        "IF",
        "IF — the second value if the condition holds, else the top",
        "TIME 10 GT 5 0 IF",
    ),
    ("DUP", "DUP — copy the top value", "1 DUP"),
    ("DROP", "DROP — discard the top value", "1 DROP"),
    ("SWAP", "SWAP — exchange the top two values", "1 2 SWAP"),
    (
        "PRINT",
        "PRINT — write the top value to the output",
        "'hello' PRINT",
    ),
    (
        "MAP",
        "MAP — run a block on each element",
        "[ 1 2 ] [ DUP MUL ] MAP",
    ),
    ("DEF", "DEF — define a User word", "[ 0.2 ] 'TAX-RATE' DEF"),
    ("DEL", "DEL — delete a User word", "'TAX-RATE' DEL"),
    (
        "TANK",
        "TANK — declare a tank (a stock, an integrator) and its level",
        "100 'BATH' TANK",
    ),
    (
        "PIPE",
        "PIPE — connect two tanks with a flow whose rate a block computes",
        "[ BATH 0.1 MUL ] 'BATH' 'OUTSIDE' 'DRAIN' PIPE",
    ),
    (
        "CAM",
        "CAM — read a value off a piecewise-linear curve, like MONIAC's cams",
        "BATH [ 0 0 100 1 ] CAM",
    ),
    ("TIME", "TIME — the machine's clock", "TIME PRINT"),
    (
        "RUN",
        "RUN — let the machine flow for a duration in steps of dt",
        "30 0.1 RUN",
    ),
    (
        "REWIND",
        "REWIND — refill every tank to its declared level and reset the clock",
        "REWIND",
    ),
];

fn is_core(name: &str) -> bool {
    CORE_WORDS.iter().any(|(n, _, _)| *n == name)
}

/// Parse source into the values a run executes, with `[ ... ]` already built.
pub fn parse(source: &str) -> Result<Vec<Value>, ExecError> {
    let tokens = tokenize(source).map_err(|m| ExecError::new("syntaxError", m))?;
    let mut groups: Vec<Vec<Value>> = vec![Vec::new()];
    for token in tokens {
        let item = match token {
            Token::Str(s) => Value::Str(s),
            Token::Word(w) if w == "[" => {
                groups.push(Vec::new());
                continue;
            }
            Token::Word(w) if w == "]" => {
                if groups.len() == 1 {
                    return Err(ExecError::new("syntaxError", "unmatched ]"));
                }
                Value::Vector(groups.pop().unwrap())
            }
            Token::Word(w) => match w.to_uppercase().as_str() {
                "TRUE" => Value::Bool(true),
                "FALSE" => Value::Bool(false),
                "NIL" => Value::Nil,
                upper => match parse_number(&w) {
                    Some(x) => Value::Number(x),
                    None => Value::Symbol(upper.to_string()),
                },
            },
        };
        groups.last_mut().unwrap().push(item);
    }
    if groups.len() > 1 {
        return Err(ExecError::new("syntaxError", "unclosed ["));
    }
    Ok(groups.pop().unwrap())
}

/// The session state the playground carries between runs and into its
/// worker: the stack and the machine (`snapshot_stack`).
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub stack: Vec<Value>,
    #[serde(default)]
    pub model: Model,
}

/// Flows already computed at the current levels, so a pipe that reads
/// another pipe neither evaluates it twice nor loops forever.
#[derive(Debug, Clone, Default)]
struct FlowMemo {
    valid: bool,
    rates: Vec<Option<f64>>,
    busy: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct Interpreter {
    pub stack: Vec<Value>,
    pub words: BTreeMap<String, UserWord>,
    pub model: Model,
    output: String,
    max_steps: u64,
    steps: u64,
    depth: usize,
    changes: Vec<String>,
    /// Inside a rate block, where only computing is allowed.
    in_rate: bool,
    memo: FlowMemo,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            stack: Vec::new(),
            words: BTreeMap::new(),
            model: Model::default(),
            output: String::new(),
            max_steps: DEFAULT_MAX_STEPS,
            steps: 0,
            depth: 0,
            changes: Vec::new(),
            in_rate: false,
            memo: FlowMemo::default(),
        }
    }

    pub fn max_steps(&self) -> u64 {
        self.max_steps
    }

    pub fn set_max_steps(&mut self, steps: u64) {
        self.max_steps = if steps == 0 { DEFAULT_MAX_STEPS } else { steps };
    }

    /// Run `source`. A failed run leaves the stack, dictionary and machine as
    /// they were before it started; output printed before the failure is kept.
    pub fn execute(&mut self, source: &str) -> Result<(), ExecError> {
        self.steps = 0;
        self.depth = 0;
        self.in_rate = false;
        self.memo.valid = false;
        self.changes.clear();
        let program = parse(source)?;
        let stack_before = self.stack.clone();
        let words_before = self.words.clone();
        let model_before = self.model.clone();
        let result = self.run(&program);
        if result.is_err() {
            self.stack = stack_before;
            self.words = words_before;
            self.model = model_before;
        }
        result
    }

    /// Text written by `PRINT` and `RUN` since the last call.
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    /// Words defined or deleted by the most recent run.
    pub fn changes_this_run(&self) -> &[String] {
        &self.changes
    }

    pub fn reset(&mut self) {
        let max_steps = self.max_steps;
        *self = Interpreter::new();
        self.max_steps = max_steps;
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            stack: self.stack.clone(),
            model: self.model.clone(),
        }
    }

    pub fn restore(&mut self, snapshot: Snapshot) {
        self.stack = snapshot.stack;
        self.model = snapshot.model;
        self.memo.valid = false;
    }

    /// What still uses `name`: a User word that calls it or a pipe whose
    /// rate reads it.
    pub fn user_of(&self, name: &str) -> Option<String> {
        if let Some((caller, _)) = self
            .words
            .iter()
            .find(|(n, w)| *n != name && mentions(&w.body, name))
        {
            return Some(caller.clone());
        }
        self.model
            .pipes
            .iter()
            .find(|p| p.name != name && mentions(&p.rate, name))
            .map(|p| format!("pipe {}", p.name))
    }

    /// Why `name` cannot name something new, if it cannot. `own` is the
    /// namespace the new thing lives in, which it may replace.
    fn name_conflict(&self, name: &str, own: &str) -> Option<String> {
        if name.is_empty() || parse_number(name).is_some() || name == OUTSIDE || is_core(name) {
            return Some(format!("{name} cannot be defined"));
        }
        let taken = [
            ("word", self.words.contains_key(name)),
            ("tank", self.model.tank(name).is_some()),
            ("pipe", self.model.pipe(name).is_some()),
        ];
        taken
            .iter()
            .find(|(kind, used)| *used && *kind != own)
            .map(|(kind, _)| format!("{name} is already a {kind}"))
    }

    /// Define a word from its definition text, as `restore_user_words` needs.
    pub fn define(
        &mut self,
        name: &str,
        definition: &str,
        description: Option<String>,
    ) -> Result<(), String> {
        let name = name.to_uppercase();
        if let Some(reason) = self.name_conflict(&name, "word") {
            return Err(reason);
        }
        let body = parse(definition).map_err(|e| e.message)?;
        self.words.insert(name, UserWord { body, description });
        Ok(())
    }

    fn run(&mut self, items: &[Value]) -> Result<(), ExecError> {
        for item in items {
            self.steps += 1;
            if self.steps > self.max_steps {
                return Err(ExecError::new("resourceLimit", "step limit exceeded"));
            }
            match item {
                Value::Symbol(name) => self.call(name)?,
                other => self.stack.push(other.clone()),
            }
        }
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, ExecError> {
        self.stack
            .pop()
            .ok_or_else(|| ExecError::new("stackUnderflow", "stack underflow"))
    }

    fn pop_string(&mut self) -> Result<String, ExecError> {
        match self.pop()? {
            Value::Str(s) => Ok(s.to_uppercase()),
            other => Err(err(format!("expected a name, got {}", other.display()))),
        }
    }

    fn pop_number(&mut self) -> Result<f64, ExecError> {
        match self.pop()? {
            Value::Number(x) => Ok(x),
            other => Err(err(format!("expected a number, got {}", other.display()))),
        }
    }

    fn push_number(&mut self, x: f64) -> Result<(), ExecError> {
        if !x.is_finite() {
            return Err(err("number out of range"));
        }
        self.stack.push(Value::Number(x));
        Ok(())
    }

    /// Building and running the machine is refused inside a rate block: a
    /// rate only reads the machine.
    fn outside_rate(&self, word: &str) -> Result<(), ExecError> {
        if self.in_rate {
            return Err(err(format!("{word} cannot be used inside a rate block")));
        }
        Ok(())
    }

    fn call(&mut self, name: &str) -> Result<(), ExecError> {
        match name {
            "ADD" => self.binary(|a, b| Ok(a + b)),
            "SUB" => self.binary(|a, b| Ok(a - b)),
            "MUL" => self.binary(|a, b| Ok(a * b)),
            "DIV" => self.binary(|a, b| {
                if b == 0.0 {
                    Err("division by zero".to_string())
                } else {
                    Ok(a / b)
                }
            }),
            "MIN" => self.binary(|a, b| Ok(a.min(b))),
            "MAX" => self.binary(|a, b| Ok(a.max(b))),
            "EQ" => {
                let (b, a) = (self.pop()?, self.pop()?);
                self.stack.push(Value::Bool(a == b));
                Ok(())
            }
            "LT" | "GT" => {
                let (b, a) = (self.pop_number()?, self.pop_number()?);
                let holds = if name == "LT" { a < b } else { a > b };
                self.stack.push(Value::Bool(holds));
                Ok(())
            }
            "IF" => {
                let (otherwise, then, condition) = (self.pop()?, self.pop()?, self.pop()?);
                match condition {
                    Value::Bool(holds) => {
                        self.stack.push(if holds { then } else { otherwise });
                        Ok(())
                    }
                    other => Err(err(format!(
                        "IF needs TRUE or FALSE, got {}",
                        other.display()
                    ))),
                }
            }
            "DUP" => {
                let top = self.pop()?;
                self.stack.push(top.clone());
                self.stack.push(top);
                Ok(())
            }
            "DROP" => self.pop().map(|_| ()),
            "SWAP" => {
                let (b, a) = (self.pop()?, self.pop()?);
                self.stack.push(b);
                self.stack.push(a);
                Ok(())
            }
            "PRINT" => {
                self.outside_rate(name)?;
                let text = match self.pop()? {
                    Value::Str(s) => s,
                    other => other.display(),
                };
                self.output.push_str(&text);
                self.output.push('\n');
                Ok(())
            }
            "MAP" => {
                let (block, target) = (self.pop()?, self.pop()?);
                let (Value::Vector(block), Value::Vector(items)) = (block, target) else {
                    return Err(err("MAP takes a vector and a block"));
                };
                let base = self.stack.len();
                let mut results = Vec::with_capacity(items.len());
                for item in items {
                    self.stack.push(item);
                    self.run(&block)?;
                    if self.stack.len() != base + 1 {
                        return Err(err("a MAP block must leave exactly one value"));
                    }
                    results.push(self.pop()?);
                }
                self.stack.push(Value::Vector(results));
                Ok(())
            }
            "DEF" => {
                self.outside_rate(name)?;
                let name = self.pop_string()?;
                let Value::Vector(body) = self.pop()? else {
                    return Err(err("DEF takes a block and a name"));
                };
                if let Some(reason) = self.name_conflict(&name, "word") {
                    return Err(ExecError::new("definitionConflict", reason));
                }
                self.words.insert(
                    name.clone(),
                    UserWord {
                        body,
                        description: None,
                    },
                );
                self.memo.valid = false;
                self.output.push_str(&format!("Defined word: {name}\n"));
                self.changes.push(name);
                Ok(())
            }
            "DEL" => {
                self.outside_rate(name)?;
                let name = self.pop_string()?;
                if !self.words.contains_key(&name) {
                    return Err(err(format!("{name} is not a User word")));
                }
                if let Some(user) = self.user_of(&name) {
                    return Err(ExecError::new(
                        "definitionConflict",
                        format!("{name} is still used by {user}"),
                    ));
                }
                self.words.remove(&name);
                self.output.push_str(&format!("Deleted word: {name}\n"));
                self.changes.push(name);
                Ok(())
            }
            "TANK" => {
                self.outside_rate(name)?;
                let name = self.pop_string()?;
                let level = self.pop_number()?;
                if let Some(reason) = self.name_conflict(&name, "tank") {
                    return Err(ExecError::new("definitionConflict", reason));
                }
                let tank = Tank {
                    name: name.clone(),
                    level,
                    initial: level,
                };
                match self.model.tank(&name) {
                    Some(i) => self.model.tanks[i] = tank,
                    None => self.model.tanks.push(tank),
                }
                self.memo.valid = false;
                self.output
                    .push_str(&format!("Tank {name} = {}\n", format_number(level)));
                Ok(())
            }
            "PIPE" => self.pipe(),
            "CAM" => {
                let Value::Vector(curve) = self.pop()? else {
                    return Err(err("CAM takes a number and a curve [ x0 y0 x1 y1 ... ]"));
                };
                let x = self.pop_number()?;
                let y = cam(x, &curve).map_err(err)?;
                self.push_number(y)
            }
            "TIME" => self.push_number(self.model.time),
            "RUN" => self.simulate(),
            "REWIND" => {
                self.outside_rate(name)?;
                for tank in &mut self.model.tanks {
                    tank.level = tank.initial;
                }
                self.model.time = 0.0;
                self.memo.valid = false;
                self.output.push_str("Rewound to t = 0\n");
                Ok(())
            }
            _ => {
                if let Some(i) = self.model.tank(name) {
                    let level = self.model.tanks[i].level;
                    return self.push_number(level);
                }
                if let Some(i) = self.model.pipe(name) {
                    let rate = self.flow(i)?;
                    return self.push_number(rate);
                }
                match self.words.get(name) {
                    Some(word) => {
                        let body = word.body.clone();
                        self.nested(|interp| interp.run(&body))
                    }
                    None => Err(ExecError::new(
                        "unknownWord",
                        format!("unknown word: {name}"),
                    )),
                }
            }
        }
    }

    fn nested<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ExecError>,
    ) -> Result<T, ExecError> {
        if self.depth >= MAX_CALL_DEPTH {
            return Err(ExecError::new("resourceLimit", "call depth limit exceeded"));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    /// `[ rate ] 'FROM' 'TO' 'NAME' PIPE`
    fn pipe(&mut self) -> Result<(), ExecError> {
        self.outside_rate("PIPE")?;
        let name = self.pop_string()?;
        let to = self.pop_string()?;
        let from = self.pop_string()?;
        let Value::Vector(rate) = self.pop()? else {
            return Err(err("PIPE takes a rate block, two tanks and a name"));
        };
        if let Some(reason) = self.name_conflict(&name, "pipe") {
            return Err(ExecError::new("definitionConflict", reason));
        }
        let end = |tank: &str| -> Result<Option<usize>, ExecError> {
            if tank == OUTSIDE {
                return Ok(None);
            }
            self.model
                .tank(tank)
                .map(Some)
                .ok_or_else(|| err(format!("{tank} is not a tank")))
        };
        let (from, to) = (end(&from)?, end(&to)?);
        if from == to {
            return Err(err("a pipe must connect two different tanks"));
        }
        let pipe = Pipe {
            name: name.clone(),
            rate,
            from,
            to,
        };
        match self.model.pipe(&name) {
            Some(i) => self.model.pipes[i] = pipe,
            None => self.model.pipes.push(pipe),
        }
        self.memo.valid = false;
        self.output.push_str(&format!(
            "Pipe {name}: {} → {}\n",
            self.model.end_name(from),
            self.model.end_name(to)
        ));
        Ok(())
    }

    /// The block's single number, computed on a stack of its own.
    fn eval_rate(&mut self, block: &[Value]) -> Result<f64, ExecError> {
        let saved = std::mem::take(&mut self.stack);
        let was_in_rate = std::mem::replace(&mut self.in_rate, true);
        let result = self.nested(|interp| interp.run(block));
        self.in_rate = was_in_rate;
        let left = std::mem::replace(&mut self.stack, saved);
        result?;
        match left.as_slice() {
            [Value::Number(x)] => Ok(*x),
            _ => Err(err("a rate block must leave exactly one number")),
        }
    }

    /// The flow through pipe `i` at the current levels. A pipe out of an
    /// empty tank carries nothing: water cannot be drawn from a dry tank.
    fn flow(&mut self, i: usize) -> Result<f64, ExecError> {
        if !self.memo.valid {
            let n = self.model.pipes.len();
            self.memo = FlowMemo {
                valid: true,
                rates: vec![None; n],
                busy: vec![false; n],
            };
        }
        if let Some(rate) = self.memo.rates[i] {
            return Ok(rate);
        }
        let pipe = self.model.pipes[i].clone();
        if self.memo.busy[i] {
            return Err(err(format!("pipe {} depends on its own flow", pipe.name)));
        }
        self.memo.busy[i] = true;
        let rate = self.eval_rate(&pipe.rate);
        self.memo.busy[i] = false;
        // The innermost pipe names the failure; the pipes reading it do not.
        let rate = rate.map_err(|e| match e.message.starts_with("pipe ") {
            true => e,
            false => ExecError::new(e.category, format!("pipe {}: {e}", pipe.name)),
        })?;
        if !rate.is_finite() {
            return Err(err(format!("pipe {}: the flow is out of range", pipe.name)));
        }
        let source = if rate >= 0.0 { pipe.from } else { pipe.to };
        let dry = source.is_some_and(|t| self.model.tanks[t].level <= 0.0);
        let rate = if dry { 0.0 } else { rate };
        self.memo.rates[i] = Some(rate);
        Ok(rate)
    }

    /// How fast each tank's level changes at `levels` and time `t`.
    fn derivative(&mut self, levels: &[f64], t: f64) -> Result<Vec<f64>, ExecError> {
        self.model.set_levels(levels);
        self.model.time = t;
        self.memo.valid = false;
        let mut change = vec![0.0; levels.len()];
        for i in 0..self.model.pipes.len() {
            let rate = self.flow(i)?;
            let Pipe { from, to, .. } = self.model.pipes[i];
            if let Some(a) = from {
                change[a] -= rate;
            }
            if let Some(b) = to {
                change[b] += rate;
            }
        }
        Ok(change)
    }

    /// `duration dt RUN`: integrate the machine with fourth-order Runge–Kutta
    /// and report each tank's course.
    fn simulate(&mut self) -> Result<(), ExecError> {
        self.outside_rate("RUN")?;
        let dt = self.pop_number()?;
        let duration = self.pop_number()?;
        if dt <= 0.0 || duration < 0.0 {
            return Err(err("RUN needs a duration of 0 or more and a positive dt"));
        }
        if self.model.tanks.is_empty() {
            return Err(err("RUN needs at least one TANK"));
        }
        let steps = (duration / dt - 1e-9).ceil().max(0.0);
        if steps > MAX_RUN_STEPS as f64 {
            return Err(ExecError::new(
                "resourceLimit",
                format!("RUN would take more than {MAX_RUN_STEPS} steps; use a larger dt"),
            ));
        }
        let steps = steps as usize;
        let start = self.model.time;
        let mut y = self.model.levels();
        let mut history: Vec<Vec<f64>> = y.iter().map(|&level| vec![level]).collect();
        let axpy = |y: &[f64], k: &[f64], h: f64| -> Vec<f64> {
            y.iter().zip(k).map(|(a, b)| a + h * b).collect()
        };
        for step in 0..steps {
            let t = start + step as f64 * dt;
            let h = dt.min(start + duration - t);
            let k1 = self.derivative(&y, t)?;
            let k2 = self.derivative(&axpy(&y, &k1, h / 2.0), t + h / 2.0)?;
            let k3 = self.derivative(&axpy(&y, &k2, h / 2.0), t + h / 2.0)?;
            let k4 = self.derivative(&axpy(&y, &k3, h), t + h)?;
            for (i, level) in y.iter_mut().enumerate() {
                *level += h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
            }
            if y.iter().any(|x| !x.is_finite()) {
                return Err(err(format!(
                    "the machine overflowed at t = {}",
                    format_number(t + h)
                )));
            }
            for (series, &level) in history.iter_mut().zip(&y) {
                series.push(level);
            }
        }
        self.model.set_levels(&y);
        self.model.time = start + duration;
        self.memo.valid = false;
        self.report(start, dt, steps, &history)
    }

    fn report(
        &mut self,
        start: f64,
        dt: f64,
        steps: usize,
        history: &[Vec<f64>],
    ) -> Result<(), ExecError> {
        let mut out = format!(
            "RUN t = {} → {}  (dt {}, {steps} steps)\n",
            format_number(start),
            format_number(self.model.time),
            format_number(dt),
        );
        let width = self
            .model
            .tanks
            .iter()
            .map(|t| t.name.chars().count())
            .chain(self.model.pipes.iter().map(|p| p.name.chars().count()))
            .max()
            .unwrap_or(0);
        for (tank, series) in self.model.tanks.iter().zip(history) {
            let first = series.first().copied().unwrap_or(tank.level);
            out.push_str(&format!(
                "  {:<width$}  {:>9} {} {}\n",
                tank.name,
                brief(first),
                sparkline(series),
                brief(tank.level),
            ));
        }
        if !self.model.pipes.is_empty() {
            out.push_str(&format!(
                "  flows at t = {}\n",
                format_number(self.model.time)
            ));
            let routes: Vec<String> = self
                .model
                .pipes
                .iter()
                .map(|p| {
                    format!(
                        "{} → {}",
                        self.model.end_name(p.from),
                        self.model.end_name(p.to)
                    )
                })
                .collect();
            let route_width = routes.iter().map(|r| r.chars().count()).max().unwrap_or(0);
            for (i, route) in routes.iter().enumerate() {
                let rate = self.flow(i)?;
                out.push_str(&format!(
                    "  {:<width$}  {route:<route_width$}  {:>9}\n",
                    self.model.pipes[i].name,
                    brief(rate),
                ));
            }
        }
        let total = |levels: Vec<f64>| brief(levels.iter().sum());
        out.push_str(&format!(
            "  total in tanks  {} → {}\n",
            total(history.iter().map(|s| s[0]).collect()),
            total(self.model.levels()),
        ));
        self.output.push_str(&out);
        Ok(())
    }

    /// Arithmetic on two numbers, or element by element when either side is
    /// a vector (a number pairs with every element).
    fn binary(&mut self, op: fn(f64, f64) -> Result<f64, String>) -> Result<(), ExecError> {
        let (b, a) = (self.pop()?, self.pop()?);
        let result = lift(&a, &b, op).map_err(err)?;
        self.stack.push(result);
        Ok(())
    }
}

/// A number for the `RUN` report, to two decimal places.
fn brief(x: f64) -> String {
    let text = format!("{x:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// `x` read off the piecewise-linear curve through `[ x0 y0 x1 y1 ... ]`,
/// held flat beyond its ends.
fn cam(x: f64, curve: &[Value]) -> Result<f64, String> {
    let numbers: Vec<f64> = curve
        .iter()
        .map(|v| match v {
            Value::Number(n) => Ok(*n),
            other => Err(format!(
                "a cam curve holds numbers, not {}",
                other.display()
            )),
        })
        .collect::<Result<_, _>>()?;
    if numbers.is_empty() || !numbers.len().is_multiple_of(2) {
        return Err("a cam curve is pairs of x and y: [ x0 y0 x1 y1 ... ]".to_string());
    }
    let points: Vec<(f64, f64)> = numbers.chunks(2).map(|p| (p[0], p[1])).collect();
    if points.windows(2).any(|w| w[1].0 <= w[0].0) {
        return Err("a cam curve's x values must increase".to_string());
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    if x <= first.0 {
        return Ok(first.1);
    }
    if x >= last.0 {
        return Ok(last.1);
    }
    let segment = points.windows(2).find(|w| x <= w[1].0).unwrap();
    let ((x0, y0), (x1, y1)) = (segment[0], segment[1]);
    Ok(y0 + (y1 - y0) * (x - x0) / (x1 - x0))
}

fn lift(a: &Value, b: &Value, op: fn(f64, f64) -> Result<f64, String>) -> Result<Value, String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let z = op(*x, *y)?;
            if z.is_finite() {
                Ok(Value::Number(z))
            } else {
                Err("number out of range".to_string())
            }
        }
        (Value::Vector(xs), Value::Vector(ys)) => {
            if xs.len() != ys.len() {
                return Err("vectors differ in length".to_string());
            }
            xs.iter()
                .zip(ys)
                .map(|(x, y)| lift(x, y, op))
                .collect::<Result<_, _>>()
                .map(Value::Vector)
        }
        (Value::Vector(xs), y) => xs
            .iter()
            .map(|x| lift(x, y, op))
            .collect::<Result<_, _>>()
            .map(Value::Vector),
        (x, Value::Vector(ys)) => ys
            .iter()
            .map(|y| lift(x, y, op))
            .collect::<Result<_, _>>()
            .map(Value::Vector),
        _ => Err(format!(
            "cannot do arithmetic on {} and {}",
            a.display(),
            b.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Interpreter {
        let mut interp = Interpreter::new();
        interp.execute(source).unwrap();
        interp
    }

    fn top(source: &str) -> String {
        run(source).stack.last().unwrap().display()
    }

    fn level(interp: &Interpreter, tank: &str) -> f64 {
        interp.model.tanks[interp.model.tank(tank).unwrap()].level
    }

    #[test]
    fn arithmetic_lifts_over_vectors() {
        assert_eq!(top("1 4 DIV 1 4 DIV ADD"), "0.5");
        assert_eq!(top("[ 1 2 3 ] 10 MUL"), "[ 10 20 30 ]");
        assert_eq!(top("[ 1 2 ] [ 3 4 ] ADD"), "[ 4 6 ]");
        assert_eq!(top("3 5 MIN 4 MAX"), "4");
        assert_eq!(top("2 1 GT 'yes' 'no' IF"), "'yes'");
    }

    #[test]
    fn user_words_and_map() {
        let interp = run("[ DUP MUL ] 'SQUARE' DEF [ 1 2 3 ] [ SQUARE ] MAP");
        assert_eq!(interp.stack.last().unwrap().display(), "[ 1 4 9 ]");
        assert_eq!(interp.words["SQUARE"].definition(), "DUP MUL");
    }

    #[test]
    fn a_failed_run_changes_nothing() {
        let mut interp = run("1");
        let error = interp
            .execute("2 [ X ] 'A' DEF 5 'T' TANK 0 DIV")
            .unwrap_err();
        assert_eq!(error.message, "division by zero");
        assert_eq!(interp.stack, vec![Value::Number(1.0)]);
        assert!(interp.words.is_empty());
        assert!(interp.model.tanks.is_empty());
        assert_eq!(interp.changes_this_run(), ["A"]);
    }

    #[test]
    fn del_refuses_a_word_still_in_use() {
        let mut interp = run("[ 1 ] 'A' DEF [ A ] 'B' DEF [ 2 ] 'C' DEF");
        assert_eq!(
            interp.execute("'A' DEL").unwrap_err().category,
            "definitionConflict"
        );
        interp
            .execute("1 'T' TANK [ C ] 'T' 'OUTSIDE' 'P' PIPE")
            .unwrap();
        assert_eq!(
            interp.execute("'C' DEL").unwrap_err().message,
            "C is still used by pipe P"
        );
        interp.execute("'B' DEL 'A' DEL").unwrap();
        assert_eq!(interp.words.len(), 1);
    }

    #[test]
    fn step_limit_stops_runaway_recursion() {
        let mut interp = run("[ LOOP ] 'LOOP' DEF");
        assert_eq!(
            interp.execute("LOOP").unwrap_err().message,
            "call depth limit exceeded"
        );
        interp.set_max_steps(10);
        assert_eq!(
            interp.execute("1 1 1 1 1 1 1 1 1 1 1").unwrap_err().message,
            "step limit exceeded"
        );
    }

    #[test]
    fn a_constant_inflow_integrates_exactly() {
        let interp = run("0 'T' TANK [ 2 ] 'OUTSIDE' 'T' 'TAP' PIPE 10 0.5 RUN");
        assert!((level(&interp, "T") - 20.0).abs() < 1e-9);
        assert_eq!(interp.model.time, 10.0);
    }

    #[test]
    fn exponential_decay_matches_the_closed_form() {
        let interp = run("100 'T' TANK [ T 0.5 MUL ] 'T' 'OUTSIDE' 'DRAIN' PIPE 4 0.01 RUN");
        let expected = 100.0 * (-2.0f64).exp();
        assert!((level(&interp, "T") - expected).abs() < 1e-6);
    }

    #[test]
    fn pipes_between_tanks_conserve_the_total() {
        let interp = run("60 'A' TANK 40 'B' TANK
             [ A 0.3 MUL ] 'A' 'B' 'AB' PIPE
             [ B 0.1 MUL ] 'B' 'A' 'BA' PIPE
             50 0.1 RUN");
        let total = level(&interp, "A") + level(&interp, "B");
        assert!((total - 100.0).abs() < 1e-9);
        // The levels settle where the two flows balance: 0.3 A = 0.1 B.
        assert!((level(&interp, "A") - 25.0).abs() < 1e-3);
    }

    #[test]
    fn a_dry_tank_stops_its_outflow() {
        let interp = run("1 'T' TANK [ 5 ] 'T' 'OUTSIDE' 'DRAIN' PIPE 10 0.01 RUN");
        assert!(level(&interp, "T").abs() < 0.06);
    }

    #[test]
    fn a_pipe_can_read_another_pipes_flow() {
        let mut interp = run("100 'A' TANK 0 'B' TANK 0 'C' TANK
             [ 10 ] 'A' 'B' 'MAIN' PIPE
             [ MAIN 0.5 MUL ] 'A' 'C' 'SIDE' PIPE");
        interp.execute("SIDE").unwrap();
        assert_eq!(interp.stack, vec![Value::Number(5.0)]);
        let error = interp
            .execute("[ LOOP ] 'A' 'B' 'LOOP' PIPE 1 1 RUN")
            .unwrap_err();
        assert_eq!(error.message, "pipe LOOP depends on its own flow");
    }

    #[test]
    fn rerunning_a_script_replaces_tanks_and_pipes() {
        let script = "5 'T' TANK [ 1 ] 'OUTSIDE' 'T' 'TAP' PIPE 1 1 RUN";
        let mut interp = run(script);
        interp.execute(script).unwrap();
        assert_eq!(interp.model.pipes.len(), 1);
        assert!((level(&interp, "T") - 6.0).abs() < 1e-9);
        interp.execute("REWIND").unwrap();
        assert_eq!(level(&interp, "T"), 5.0);
        assert_eq!(interp.model.time, 0.0);
    }

    #[test]
    fn time_and_cams_shape_a_rate() {
        assert_eq!(top("5 [ 0 0 10 1 ] CAM"), "0.5");
        assert_eq!(top("-5 [ 0 0 10 1 ] CAM"), "0");
        let interp = run("0 'T' TANK [ TIME 5 LT 1 0 IF ] 'OUTSIDE' 'T' 'TAP' PIPE 10 0.01 RUN");
        assert!((level(&interp, "T") - 5.0).abs() < 0.02);
    }

    #[test]
    fn rate_blocks_only_compute() {
        let mut interp = run("1 'T' TANK [ 2 'X' TANK 1 ] 'T' 'OUTSIDE' 'P' PIPE");
        assert_eq!(
            interp.execute("1 1 RUN").unwrap_err().message,
            "pipe P: TANK cannot be used inside a rate block"
        );
        let mut interp = run("1 'T' TANK [ 1 2 ] 'T' 'OUTSIDE' 'P' PIPE");
        assert_eq!(
            interp.execute("P").unwrap_err().message,
            "pipe P: a rate block must leave exactly one number"
        );
    }

    #[test]
    fn names_live_in_one_namespace() {
        let mut interp = run("1 'T' TANK");
        assert_eq!(
            interp.execute("[ 1 ] 'T' DEF").unwrap_err().message,
            "T is already a tank"
        );
        assert!(interp.execute("1 'OUTSIDE' TANK").is_err());
        assert!(interp.execute("1 'RUN' TANK").is_err());
    }

    #[test]
    fn the_snapshot_carries_the_machine() {
        let interp = run("7 'T' TANK 3");
        let text = serde_json::to_string(&interp.snapshot()).unwrap();
        let mut other = Interpreter::new();
        other.restore(serde_json::from_str(&text).unwrap());
        other.execute("T ADD").unwrap();
        assert_eq!(other.stack, vec![Value::Number(10.0)]);
    }
}

#[cfg(test)]
mod example_tests {
    use super::*;

    #[test]
    fn the_moniac_example_runs_and_conserves_its_water() {
        let mut interp = Interpreter::new();
        interp
            .execute(include_str!("../../examples/moniac.alg"))
            .unwrap();
        let total: f64 = interp.model.levels().iter().sum();
        assert!((total - 1000.0).abs() < 1e-6, "total drifted to {total}");
        assert_eq!(interp.model.time, 100.0);
        let output = interp.take_output();
        println!("{output}");
        assert_eq!(output.matches("RUN t =").count(), 2);
    }

    #[test]
    fn the_readme_bath_reports_its_course() {
        let mut interp = Interpreter::new();
        interp
            .execute(
                "100 'BATH' TANK [ 5 ] 'OUTSIDE' 'BATH' 'TAP' PIPE
                 [ BATH 0.1 MUL ] 'BATH' 'OUTSIDE' 'DRAIN' PIPE 30 0.1 RUN",
            )
            .unwrap();
        let output = interp.take_output();
        println!("{output}");
        assert!(output.contains("total in tanks  100 → 52.49"));
    }
}
