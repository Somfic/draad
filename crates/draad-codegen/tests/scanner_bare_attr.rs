mod common;

#[test]
fn aggregator_wires_apply_routes_for_bare_attribute() {
    let root = common::fresh_root("bare");
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
        out.contains("rpc_router"),
        "expected rpc_router fn in:\n{out}"
    );
    assert!(
        out.contains("crate::api::search::__draad_search_apply_routes(router)"),
        "expected aggregator to chain search::apply_routes:\n{out}"
    );
}

#[test]
fn module_chunk_emits_route_and_handler_for_bare_attribute() {
    let out = common::module_rust(
        r#"
#[api(namespace = "search")]
pub trait SearchApi {
    async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
}
"#,
        "search",
    );

    assert!(
        out.contains(".route(\"/search/query\""),
        "expected route registration in:\n{out}"
    );
    assert!(
        out.contains("async fn __search_query"),
        "expected handler in:\n{out}"
    );
    assert!(
        out.contains("pub fn apply_routes"),
        "expected apply_routes fn in:\n{out}"
    );
}
