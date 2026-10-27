#![allow(clippy::unwrap_used)]

use codemycel_core::{RepoPath, SemanticPath};

fn repo(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn round_trip(text: &str) -> SemanticPath {
    let parsed: SemanticPath = text.parse().unwrap();
    assert_eq!(parsed.to_string(), text);
    parsed
}

#[test]
fn simple_path_round_trips() {
    let built =
        SemanticPath::symbol(&repo("crates/core/src/store.rs"), &["Store"], "open", None).unwrap();
    assert_eq!(built.to_string(), "crates/core/src/store.rs::Store::open");

    let parsed = round_trip("crates/core/src/store.rs::Store::open");
    assert_eq!(parsed, built);
    assert_eq!(parsed.file(), Some("crates/core/src/store.rs"));
    assert_eq!(parsed.containers(), vec!["Store"]);
    assert_eq!(parsed.name(), "open");
    assert_eq!(parsed.disambiguator(), None);
}

#[test]
fn trait_impl_paths_round_trip() {
    let fmt = round_trip("src/fmt.rs::<Vec<Foo> as Trait>::m");
    assert_eq!(fmt.file(), Some("src/fmt.rs"));
    assert_eq!(fmt.containers(), vec!["<Vec<Foo> as Trait>"]);
    assert_eq!(fmt.name(), "m");

    let blanket = round_trip("src/x.rs::<T as Y>::method");
    assert_eq!(blanket.containers(), vec!["<T as Y>"]);
    assert_eq!(blanket.name(), "method");

    let qualified = round_trip("src/x.rs::<Foo as fmt::Display>::fmt");
    assert_eq!(qualified.containers(), vec!["<Foo as fmt::Display>"]);

    let closure = round_trip("src/x.rs::<F as Fn() -> io::Result<()>>::call");
    assert_eq!(closure.containers(), vec!["<F as Fn() -> io::Result<()>>"]);
    assert_eq!(closure.name(), "call");

    let built =
        SemanticPath::symbol(&repo("src/fmt.rs"), &["<Vec<Foo> as Trait>"], "m", None).unwrap();
    assert_eq!(built, fmt);
}

#[test]
fn suffix_is_not_part_of_name() {
    let path = round_trip("a.ts::f[2]");
    assert_eq!(path.name(), "f");
    assert_eq!(path.disambiguator(), Some(2));
    assert_eq!(
        SemanticPath::symbol(&repo("a.ts"), &[] as &[&str], "f", Some(2)).unwrap(),
        path
    );

    let nested = round_trip("a.ts::ns[1]::f");
    assert_eq!(nested.containers(), vec!["ns[1]"]);
    assert_eq!(nested.disambiguator(), None);
}

#[test]
fn same_path_different_kind_are_distinct_identities() {
    use codemycel_core::{NodeKind, NodeRef};

    let path = SemanticPath::symbol(&repo("a.ts"), &[] as &[&str], "Foo", None).unwrap();
    assert_eq!(path.to_string(), "a.ts::Foo");
    let constant = NodeRef::new(path.clone(), NodeKind::Constant);
    let alias = NodeRef::new(path, NodeKind::TypeAlias);
    assert_ne!(constant, alias);
    assert_eq!(constant.to_string(), "a.ts::Foo (constant)");
}

#[test]
fn heading_paths() {
    let built = SemanticPath::heading(&repo("docs/a.md"), "install", Some(1)).unwrap();
    assert_eq!(built.to_string(), "docs/a.md#install[1]");
    let parsed = round_trip("docs/a.md#install[1]");
    assert_eq!(parsed, built);
    assert_eq!(parsed.file(), Some("docs/a.md"));
    assert_eq!(parsed.name(), "install");
    assert_eq!(parsed.disambiguator(), Some(1));
    assert!(parsed.containers().is_empty());
}

#[test]
fn community_and_note_paths() {
    let community = SemanticPath::community("3f2a9c10").unwrap();
    assert_eq!(community.to_string(), "community:3f2a9c10");
    assert_eq!(round_trip("community:3f2a9c10"), community);
    assert_eq!(community.file(), None);
    assert_eq!(community.name(), "3f2a9c10");

    let uuid = "5b1c7e2a-0d4f-4c1e-9a7b-2f3e4d5c6b7a";
    let note = SemanticPath::note(uuid).unwrap();
    assert_eq!(note.to_string(), format!("note:{uuid}"));
    assert_eq!(round_trip(&format!("note:{uuid}")), note);
    assert_eq!(note.name(), uuid);
}

#[test]
fn file_and_package_paths() {
    let file = SemanticPath::for_file(&repo("src/main.rs")).unwrap();
    assert_eq!(file.to_string(), "src/main.rs");
    assert_eq!(file.file(), Some("src/main.rs"));
    assert_eq!(file.name(), "src/main.rs");
    assert_eq!(round_trip("src/main.rs"), file);

    let root_package = SemanticPath::for_package("Cargo.toml").unwrap();
    assert_eq!(root_package.to_string(), "Cargo.toml");
    assert_eq!(SemanticPath::for_package("serde").unwrap().name(), "serde");
    assert!(SemanticPath::for_package("").is_err());
    assert!(SemanticPath::for_file(&repo("")).is_err());
}

#[test]
fn hashes_in_file_paths_are_not_headings() {
    let file = SemanticPath::for_file(&repo("docs/C#/intro.md")).unwrap();
    assert_eq!(file.file(), Some("docs/C#/intro.md"));
    assert_eq!(file.name(), "docs/C#/intro.md");
    assert!(SemanticPath::for_file(&repo("src/#weird#")).is_ok());
    assert!(SemanticPath::for_file(&repo("docs/a.md#b")).is_err());
    assert!(SemanticPath::for_file(&repo("a::b.md")).is_err());
}

#[test]
fn const_generic_containers_keep_their_brackets() {
    let path = SemanticPath::symbol(&repo("f.rs"), &["Foo<{ A < B }>"], "m", None).unwrap();
    assert_eq!(path.containers(), vec!["Foo<{ A < B }>"]);
    assert_eq!(path.name(), "m");
    assert!(SemanticPath::symbol(&repo("f.rs"), &["Foo<"], "bar", None).is_err());
}

#[test]
fn file_segment_is_literal_and_quoted_names_are_opaque() {
    for file in [
        "src/[id.ts",
        "src/a(1.ts",
        "src/a<b.ts",
        "app/(group)/[slug]/page.tsx",
    ] {
        let path = SemanticPath::symbol(&repo(file), &["Foo"], "f", None).unwrap();
        assert_eq!(path.file(), Some(file));
        assert_eq!(path.containers(), vec!["Foo"]);
        assert_eq!(path.name(), "f");
    }
    let quoted = SemanticPath::symbol(&repo("src/a.ts"), &["Foo"], "\"<::>\"", None).unwrap();
    assert_eq!(quoted.containers(), vec!["Foo"]);
    assert_eq!(quoted.name(), "\"<::>\"");
    let lifetime =
        SemanticPath::symbol(&repo("src/a.rs"), &["<Foo<'a> as Trait>"], "m", None).unwrap();
    assert_eq!(lifetime.containers(), vec!["<Foo<'a> as Trait>"]);
    for self_type in [
        "W<'\"'>",
        "W<b'('>",
        "W<'<'>",
        "W<')'>",
        "W<'\\''>",
        "W<'a, 'b>",
    ] {
        let path = SemanticPath::symbol(&repo("src/w.rs"), &[self_type], "f", None).unwrap();
        assert_eq!(path.containers(), vec![self_type]);
        assert_eq!(path.name(), "f");
    }
}

#[test]
fn non_ascii_containers_and_names_round_trip() {
    let path =
        SemanticPath::symbol(&repo("src/a.ts"), &["Größe", "W<'é'>"], "naïve", None).unwrap();
    assert_eq!(path.containers(), vec!["Größe", "W<'é'>"]);
    assert_eq!(path.name(), "naïve");
    assert_eq!(round_trip("src/a.rs::café").name(), "café");
    assert_eq!(round_trip("src/a.ts::π").name(), "π");
}

#[test]
fn qualified_inherent_self_types_use_angle_brackets() {
    for self_type in ["<super::Foo>", "<crate::outer::Foo>", "<dyn crate::Tr>"] {
        let path = SemanticPath::symbol(&repo("src/x.rs"), &[self_type], "new", None).unwrap();
        assert_eq!(path.containers(), vec![self_type]);
        assert_eq!(path.name(), "new");
    }
    assert!(SemanticPath::symbol(&repo("src/x.rs"), &["super::Foo"], "new", None).is_err());
}

#[test]
fn display_is_stable_across_platforms() {
    let built = SemanticPath::symbol(&repo(r"src\db\store.rs"), &["Store"], "open", None).unwrap();
    assert_eq!(built.to_string(), "src/db/store.rs::Store::open");
    assert_eq!(
        SemanticPath::for_file(&repo(r".\src\db\store.rs"))
            .unwrap()
            .to_string(),
        "src/db/store.rs"
    );
}

#[test]
fn names_with_separators_are_rejected() {
    assert!(SemanticPath::symbol(&repo("a.rs"), &[] as &[&str], "a::b", None).is_err());
    assert!(SemanticPath::symbol(&repo("a.rs"), &[] as &[&str], "", None).is_err());
    assert!(SemanticPath::symbol(&repo("a.rs"), &["Foo"], "f[2]", None).is_err());
    assert!(SemanticPath::symbol(&repo(""), &[] as &[&str], "x", None).is_err());
    assert!(SemanticPath::heading(&repo("a.md"), "step[1]", None).is_err());
    assert!(SemanticPath::heading(&repo(""), "x", None).is_err());
    assert!("a.rs::".parse::<SemanticPath>().is_err());
    assert!("".parse::<SemanticPath>().is_err());
    assert_eq!(round_trip("a.ts::f[01]").name(), "f[01]");
}

#[test]
fn repo_paths_are_normalised_and_relative() {
    assert_eq!(repo(r"src\db\store.rs").as_str(), "src/db/store.rs");
    assert_eq!(repo("./src//a.rs").as_str(), "src/a.rs");
    assert_eq!(repo("src/a dir/é.rs").as_str(), "src/a dir/é.rs");
    assert_eq!(repo("").as_str(), "");
    assert!(RepoPath::new("/etc/passwd").is_err());
    assert!(RepoPath::new(r"C:\src\a.rs").is_err());
    assert!(RepoPath::new(r"\\server\share\a.rs").is_err());
    assert!(RepoPath::new("C:").is_err());
    assert_eq!(repo("a:b.txt").as_str(), "a:b.txt");
    assert!(RepoPath::new("src/../../a.rs").is_err());
}
