//! Browser-direct URL contracts. Declare a streaming/asset route once and draad
//! generates a typed TypeScript URL-builder (`api.urls.*`) plus Rust path
//! constants (`crate::urls::*`). draad never serves the bytes — mount your own
//! Axum handler against the constant, e.g. `.route(crate::urls::AVATAR, get(...))`.

#[draad::raw]
pub trait Urls {
    /// A downloadable asset. `{*path}` is a catch-all (holds slashes), so it's
    /// interpolated raw rather than URL-encoded.
    #[get("/files/{*path}")]
    fn file(path: String);

    /// A user's avatar at a given size. `id` (numeric) is interpolated raw;
    /// `size` (string) is `encodeURIComponent`'d.
    #[get("/avatar/{id}/{size}")]
    fn avatar(id: i64, size: String);
}
