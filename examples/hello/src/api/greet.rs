use crate::api::error::ApiError;
use crate::AppContext;

use draad::api;

#[api(namespace = "greet")]
pub trait GreetApi {
    /// Returns a personalized greeting. Errors with `EmptyName` if the
    /// input is blank.
    async fn hello2(&self, name: String) -> Result<String, ApiError>;

    /// Adds two numbers. Errors with `Overflow` if `a + b` doesn't fit
    /// in an `i32`.
    async fn add(&self, a: i32, b: i32) -> Result<i32, ApiError>;
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
}
