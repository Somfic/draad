use crate::api::error::ApiError;
use crate::AppContext;

use draad::api;
use draad::runtime::Conn;

#[api(namespace = "greet")]
pub trait GreetApi {
    /// Returns a personalized greeting. Errors with `EmptyName` if the
    /// input is blank.
    async fn hello2(&self, name: String) -> Result<String, ApiError>;

    /// Adds two numbers. Errors with `Overflow` if `a + b` doesn't fit
    /// in an `i32`.
    async fn add(&self, a: i32, b: i32) -> Result<i32, ApiError>;

    /// Demonstrates injecting the caller's live connection into an HTTP
    /// handler: returns the server-assigned `client_id` and pushes a
    /// `greet/pong` frame down the *same* client's WebSocket. The generated
    /// TS is `whoami(): Promise<string>` — the `conn` param is server-filled.
    /// 409s if the caller has no live socket.
    async fn whoami(&self, conn: &Conn) -> Result<String, ApiError>;
}

#[api]
impl GreetApi for AppContext {
    async fn hello2(&self, name: String) -> Result<String, ApiError> {
        if name.trim().is_empty() {
            return Err(ApiError::EmptyName);
        }
        Ok(format!("Hello, {name}!"))
    }

    async fn add(&self, a: i32, b: i32) -> Result<i32, ApiError> {
        a.checked_add(b).ok_or(ApiError::Overflow)
    }

    async fn whoami(&self, conn: &Conn) -> Result<String, ApiError> {
        conn.send("greet/pong", &());
        Ok(conn.client_id().to_string())
    }
}
