mod common;

#[test]
fn bare_method_defaults_to_post_with_json_body() {
    let root = common::fresh_root("verb-default");
    std::fs::write(
        root.join("src/search.rs"),
        r#"
use draad::{api, ty};

#[ty]
pub struct Hit { pub id: i64, pub title: String }

#[api(namespace = "search")]
pub trait SearchApi {
    async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
}
"#,
    )
    .unwrap();

    let out = common::run(&root);

    assert!(
        out.contains("Json(__args): Json<__search_query_args>"),
        "expected JSON body extractor for default-POST method:\n{out}"
    );
    assert!(
        out.contains(".route(\"/search/query\", post(__search_query))"),
        "expected POST routing fn:\n{out}"
    );
    assert!(
        !out.contains("Query(__args)"),
        "default method must not use Query extractor:\n{out}"
    );
    assert!(
        out.contains("-> Result<Json<Vec<Hit>>, MyError>"),
        "expected Result<Json<Ok>, Err> handler signature:\n{out}"
    );
    assert!(
        out.contains(".await.map(Json)"),
        "expected `.map(Json)` to wrap the Ok side:\n{out}"
    );
    assert!(
        !out.contains("Response<") && !out.contains("ok("),
        "Response<T>/ok() shim should be gone:\n{out}"
    );
}

#[test]
fn get_method_uses_query_extractor_and_get_route() {
    let root = common::fresh_root("verb-get");
    std::fs::write(
        root.join("src/files.rs"),
        r#"
use draad::api;

#[api(namespace = "files")]
pub trait FilesApi {
    #[get]
    async fn serve(&self, path: String) -> Result<Vec<u8>, MyError>;
}
"#,
    )
    .unwrap();

    let out = common::run(&root);

    assert!(
        out.contains("Query(__args): Query<__files_serve_args>"),
        "expected Query extractor for #[get]:\n{out}"
    );
    assert!(
        out.contains(".route(\"/files/serve\", get(__files_serve))"),
        "expected GET routing fn:\n{out}"
    );
    assert!(
        !out.contains("Json(__args): Json<__files_serve_args>"),
        "GET method must not use Json extractor:\n{out}"
    );
}

#[test]
fn delete_method_uses_query_extractor_and_delete_route() {
    let root = common::fresh_root("verb-delete");
    std::fs::write(
        root.join("src/hls.rs"),
        r#"
use draad::api;

#[api(namespace = "hls")]
pub trait HlsApi {
    #[delete]
    async fn stop(&self, session_id: String) -> Result<(), MyError>;
}
"#,
    )
    .unwrap();

    let out = common::run(&root);

    assert!(
        out.contains("Query(__args): Query<__hls_stop_args>"),
        "expected Query extractor for #[delete]:\n{out}"
    );
    assert!(
        out.contains(".route(\"/hls/stop\", delete(__hls_stop))"),
        "expected DELETE routing fn:\n{out}"
    );
}

#[test]
fn put_and_patch_keep_json_body() {
    let root = common::fresh_root("verb-put-patch");
    std::fs::write(
        root.join("src/items.rs"),
        r#"
use draad::api;

#[api(namespace = "items")]
pub trait ItemsApi {
    #[put]
    async fn replace(&self, id: i64, title: String) -> Result<(), MyError>;

    #[patch]
    async fn touch(&self, id: i64) -> Result<(), MyError>;
}
"#,
    )
    .unwrap();

    let out = common::run(&root);

    assert!(
        out.contains("Json(__args): Json<__items_replace_args>"),
        "PUT should keep JSON body:\n{out}"
    );
    assert!(
        out.contains(".route(\"/items/replace\", put(__items_replace))"),
        "expected PUT routing fn:\n{out}"
    );
    assert!(
        out.contains("Json(__args): Json<__items_touch_args>"),
        "PATCH should keep JSON body:\n{out}"
    );
    assert!(
        out.contains(".route(\"/items/touch\", patch(__items_touch))"),
        "expected PATCH routing fn:\n{out}"
    );
}

#[test]
fn ts_client_passes_verb_for_non_post_methods() {
    let root = common::fresh_root("verb-ts");

    std::fs::write(
        root.join("src/files.rs"),
        r#"
use draad::api;

#[api(namespace = "files")]
pub trait FilesApi {
    #[get]
    async fn serve(&self, path: String) -> Result<Vec<u8>, MyError>;

    #[delete]
    async fn drop(&self, name: String) -> Result<(), MyError>;

    async fn list(&self, prefix: String) -> Result<Vec<String>, MyError>;
}
"#,
    )
    .unwrap();

    // No `#[ty]` types in this trait, so no per-type files are needed —
    // the codegen only walks the dir when `types_in_order` is non-empty.

    let client_dir = root.join("frontend");
    draad_codegen::Config::new()
        .root(&root)
        .client_dir(&client_dir)
        .generate()
        .unwrap();

    let index = std::fs::read_to_string(client_dir.join("index.ts")).expect("index.ts");

    assert!(
        index.contains("this.rpc.call(\"files/serve\", { path }, \"GET\")"),
        "#[get] method should pass verb:\n{index}"
    );
    assert!(
        index.contains("this.rpc.call(\"files/drop\", { name }, \"DELETE\")"),
        "#[delete] method should pass verb:\n{index}"
    );
    assert!(
        index.contains("this.rpc.call(\"files/list\", { prefix });"),
        "bare (POST) method should not pass a verb arg:\n{index}"
    );
}

#[test]
#[should_panic(expected = "conflicting verb attributes")]
fn conflicting_verbs_panic() {
    let root = common::fresh_root("verb-conflict");
    std::fs::write(
        root.join("src/x.rs"),
        r#"
use draad::api;

#[api(namespace = "x")]
pub trait XApi {
    #[get]
    #[post]
    async fn weird(&self) -> Result<(), MyError>;
}
"#,
    )
    .unwrap();

    let _ = common::run(&root);
}

#[test]
#[should_panic(expected = "not query-string-safe")]
fn non_primitive_arg_on_get_panics() {
    let root = common::fresh_root("verb-non-primitive");
    std::fs::write(
        root.join("src/y.rs"),
        r#"
use draad::{api, ty};

#[ty]
pub struct Filter { pub q: String }

#[api(namespace = "y")]
pub trait YApi {
    #[get]
    async fn search(&self, conn: &Conn, filter: Filter) -> Result<(), MyError>;
}
"#,
    )
    .unwrap();

    let _ = common::run(&root);
}
