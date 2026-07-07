mod common;

#[test]
fn aggregator_wires_apply_routes_for_qualified_attribute() {
    let root = common::fresh_root("qualified");
    std::fs::write(
        root.join("src/search.rs"),
        r#"
#[draad::ty]
pub struct Hit { pub id: i64 }

#[draad::api(namespace = "search")]
pub trait SearchApi {
    async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
}
"#,
    )
    .unwrap();

    let out = common::run(&root);
    assert!(
        out.contains("crate::api::search::__draad_search_apply_routes(router)"),
        "qualified-path attribute should be detected; got:\n{out}"
    );
}

#[test]
fn module_chunk_emits_route_for_qualified_attribute() {
    let out = common::module_rust(
        r#"
#[draad::api(namespace = "search")]
pub trait SearchApi {
    async fn query(&self, q: String) -> Result<Vec<Hit>, MyError>;
}
"#,
        "search",
    );
    assert!(
        out.contains(".route(\"/search/query\""),
        "qualified-path attribute should produce a route; got:\n{out}"
    );
}
