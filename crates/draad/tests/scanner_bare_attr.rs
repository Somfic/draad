#![cfg(feature = "codegen")]

mod common;

#[test]
fn emits_route_and_router_for_bare_attribute() {
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
        out.contains("/search/query"),
        "expected route name in:\n{out}"
    );
    assert!(
        out.contains("rpc_router"),
        "expected rpc_router fn in:\n{out}"
    );
}
