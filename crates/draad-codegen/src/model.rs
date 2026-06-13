//! Internal model: structs the scanner/parser builds up, consumed by the
//! Rust and TypeScript emitters.

pub(super) struct Param {
    pub name: String,
    pub ts_type: String,
    pub rust_type: String,
    pub docs: Vec<String>,
    /// `Some` when this is an injected connection parameter (`conn: &Conn` /
    /// `conn: Option<&Conn>`) rather than a wire argument. Such params are
    /// excluded from the wire args struct and the TS client signature; the
    /// generated handler fills them from the caller's live connection.
    pub conn: Option<ConnInject>,
}

/// Marks a `&Conn` (required) / `Option<&Conn>` (optional) injected parameter.
#[derive(Copy, Clone)]
pub(super) struct ConnInject {
    pub required: bool,
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

/// One path segment of a `#[raw]` endpoint template, in source order. Drives
/// the TS encode decision: `Static` is interpolated verbatim, a non-catch-all
/// `Param` of a `String` type is `encodeURIComponent`'d, numeric or catch-all
/// params are interpolated raw.
pub(super) enum PathSeg {
    /// Literal text between params (includes the leading/trailing slashes).
    Static(String),
    /// `{name}` (catch_all = false) or `{*name}` (catch_all = true).
    Param { name: String, catch_all: bool },
}

/// One `#[get("/…")]` method on a `#[raw]` trait.
pub(super) struct RawEndpoint {
    pub rust_name: String,
    pub ts_name: String,
    /// The path template verbatim, e.g. `/api/stream/{info_hash}/{file_idx}`.
    /// Emitted as the Rust `&str` const value.
    pub path_template: String,
    /// Path params in declaration order (plain `String`/integer types).
    pub params: Vec<Param>,
    /// Parsed template, used by the TS emitter to build the interpolation.
    pub segments: Vec<PathSeg>,
    pub docs: Vec<String>,
}

/// A `#[raw]` manifest: a flat set of URL endpoints. Emits a TS `Urls` class
/// (`api.urls`) of URL-builders plus a Rust `urls` module of path constants.
pub(super) struct RawApi {
    pub class_name: String,
    pub docs: Vec<String>,
    pub methods: Vec<RawEndpoint>,
}
