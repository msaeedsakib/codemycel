#![allow(clippy::unwrap_used)]

mod harness;

use codemycel_index::Registry;
use codemycel_parse::rust::RustAdapter;
use harness::{fixture, index_fixture_with};

#[test]
fn rust_fixture_matches_expected() {
    let dir = fixture("rust/workspace-basic");
    index_fixture_with(
        &dir,
        Registry::with_adapters(vec![Box::new(RustAdapter::new())]),
    )
    .assert_matches(&dir);
}
