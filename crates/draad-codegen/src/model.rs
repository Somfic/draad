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
    /// TS form of the Ok-side return type. `"void"` for unit / no return.
    pub ret_ts: String,
    /// Rust form of the *full* return type as the user wrote it
    /// (e.g. `Result < Vec < Hit > , MyError >`). For `Result` returns
    /// the [`Method::returns_result`] flag is set and the emitter pulls
    /// the Ok side out via [`super::types::result_ok_type`].
    pub ret_rust: String,
    pub returns_result: bool,
    /// Rust form of the Err side when `returns_result` is true,
    /// otherwise `None`. Used by the rust emitter to declare the
    /// handler's `Result<Json<Ok>, Err>` return type — `Err` must
    /// implement `axum::response::IntoResponse`.
    pub err_rust: Option<String>,
    /// TS form of the same. Surfaces in `@throws {RpcError<…>}` jsdoc
    /// on the generated client method. `None` when the return is not a
    /// `Result`.
    pub err_ts: Option<String>,
    pub docs: Vec<String>,
    pub verb: Verb,
}

/// HTTP verb a method is exposed as. Default is `Post` (matches the
/// historical "everything is JSON-RPC over POST" behaviour); `#[get]` /
/// `#[put]` / `#[patch]` / `#[delete]` on a trait method overrides it.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(super) enum Verb {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Verb {
    /// Verbs that carry a request body (args go JSON-encoded). The
    /// complement carry args in the query string.
    pub(super) fn has_body(self) -> bool {
        matches!(self, Verb::Post | Verb::Put | Verb::Patch)
    }

    /// `axum::routing::<fn>` name for this verb.
    pub(super) fn axum_fn(self) -> &'static str {
        match self {
            Verb::Get => "get",
            Verb::Post => "post",
            Verb::Put => "put",
            Verb::Patch => "patch",
            Verb::Delete => "delete",
        }
    }

    /// Uppercase wire form used in the generated TS client.
    pub(super) fn ts_label(self) -> &'static str {
        match self {
            Verb::Get => "GET",
            Verb::Post => "POST",
            Verb::Put => "PUT",
            Verb::Patch => "PATCH",
            Verb::Delete => "DELETE",
        }
    }
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
