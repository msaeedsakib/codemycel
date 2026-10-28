#![allow(clippy::unwrap_used)]

mod harness;

use codemycel_core::Language;
use codemycel_index::Registry;
use harness::{fixture, fixture_copy, index_fixture_full, local_provisioner};

#[test]
fn typescript_fixture_matches_expected_after_lsp() {
    let registry = Registry::default();
    let Some(provisioner) = local_provisioner(registry.server_for(Language::TypeScript).unwrap())
    else {
        return;
    };
    let source = fixture("typescript/monorepo-basic");
    let (_temp, copy) = fixture_copy(&source);
    index_fixture_full(&copy, registry, &provisioner).assert_matches(&source);
}
