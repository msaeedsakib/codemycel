#![allow(clippy::unwrap_used)]

mod harness;

use codemycel_index::Registry;
use codemycel_parse::typescript::{JavaScriptAdapter, TypeScriptAdapter};
use harness::{fixture, index_fixture_with};

#[test]
fn typescript_fixture_matches_expected() {
    let dir = fixture("typescript/monorepo-basic");
    let registry = Registry::with_adapters(vec![
        Box::new(TypeScriptAdapter::new()),
        Box::new(JavaScriptAdapter::new()),
    ]);
    index_fixture_with(&dir, registry).assert_matches(&dir);
}
