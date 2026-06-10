mod common;

#[test]
fn emits_route_for_draad_qualified_attribute() {
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
        out.contains("/search/query"),
        "qualified-path attribute should be detected; got:\n{out}"
    );
}
