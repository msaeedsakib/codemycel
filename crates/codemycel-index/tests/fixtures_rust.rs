#![allow(clippy::unwrap_used)]

mod harness;

use harness::{assert_matches_expected, fixture};

#[test]
fn rust_fixture_matches_expected() {
    assert_matches_expected(&fixture("rust/workspace-basic"));
}
