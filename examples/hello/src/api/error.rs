//! Demo error type for the hello example. `#[ty]` makes the variant
//! shape visible to the generated TS client (inlined into `index.ts`),
//! and the `IntoResponse` impl picks the status code per variant — so a
//! frontend caller's `catch (e) { if (e instanceof RpcError) ... }`
//! sees both the HTTP status and the typed body.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use draad::ty;

#[ty]
pub enum ApiError {
    /// Caller sent an empty / whitespace-only name.
    EmptyName,
    /// Adding the two operands would overflow `i32`.
    Overflow,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self {
            ApiError::EmptyName | ApiError::Overflow => StatusCode::BAD_REQUEST,
        };
        (status, Json(self)).into_response()
    }
}
