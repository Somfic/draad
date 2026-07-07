/// Minimal publish interface required by draad event emitters.
///
/// Implement this for your event bus type and pass it to
/// `include_generated!` as the second argument. The built-in
/// `draad::runtime::EventBus` already implements this trait.
pub trait Bus: Clone {
    /// Publish `payload` to all current subscribers of `topic`.
    fn publish<T: serde::Serialize + ?Sized>(&self, topic: &str, payload: &T);
}
