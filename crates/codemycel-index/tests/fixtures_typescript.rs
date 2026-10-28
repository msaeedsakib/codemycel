#![allow(clippy::unwrap_used)]

mod harness;

use harness::{assert_matches_expected, fixture};

#[test]
fn typescript_fixture_matches_expected() {
    assert_matches_expected(&fixture("typescript/monorepo-basic"));
}
