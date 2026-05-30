//! Internal model: structs the scanner/parser builds up, consumed by the
//! Rust and TypeScript emitters.

pub(super) struct Param {
    pub name: String,
    pub ts_type: String,
    pub rust_type: String,
    pub docs: Vec<String>,
}

pub(super) struct Method {
    pub rust_name: String,
    pub ts_name: String,
    /// Wire route segment: `{namespace}_{rust_name}`.
    pub command: String,
    pub params: Vec<Param>,
    pub ret_ts: String,
    pub ret_rust: String,
    pub returns_result: bool,
    pub docs: Vec<String>,
}

pub(super) struct Api {
    pub namespace: String,
    pub module: String,
    pub class_name: String,
    pub docs: Vec<String>,
    pub methods: Vec<Method>,
}

pub(super) struct Event {
    pub rust_name: String,
    pub ts_name: String,
    pub wire: String,
    pub payload_ts: String,
    pub payload_rust: String,
    pub docs: Vec<String>,
}

pub(super) struct EventApi {
    pub namespace: String,
    pub module: String,
    pub class_name: String,
    pub docs: Vec<String>,
    pub events: Vec<Event>,
}
