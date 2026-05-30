use crate::AppContext;

use draad::api;

#[api(namespace = "greet")]
pub trait GreetApi {
    /// Returns a personalized greeting
    async fn hello(&self, name: String) -> String;

    /// Adds two numbers
    async fn add(&self, a: i32, b: i32) -> i32;
}

#[api]
impl GreetApi for AppContext {
    async fn hello(&self, name: String) -> String {
        format!("Hello, {name}!")
    }

    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }
}
