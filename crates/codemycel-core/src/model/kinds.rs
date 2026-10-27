use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// Defines a fieldless enum whose snake_case strings are part of the on-disk format.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ModelError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok($name::$variant),)+
                    _ => Err(ModelError::UnknownVariant {
                        type_name: stringify!($name),
                        value: text.to_owned(),
                    }),
                }
            }
        }
    };
}

string_enum!(Language {
    Rust => "rust",
    TypeScript => "typescript",
    JavaScript => "javascript",
});

string_enum!(
    /// Spec §6.1.
    NodeKind {
        File => "file",
        Module => "module",
        Class => "class",
        Struct => "struct",
        Enum => "enum",
        EnumMember => "enum_member",
        Trait => "trait",
        Interface => "interface",
        TypeAlias => "type_alias",
        Function => "function",
        Method => "method",
        Field => "field",
        Constant => "constant",
        Variable => "variable",
        Package => "package",
        Doc => "doc",
        Heading => "heading",
        Community => "community",
        Note => "note",
    }
);

string_enum!(
    /// Spec §6.3.
    EdgeKind {
        Contains => "contains",
        Defines => "defines",
        Imports => "imports",
        Exports => "exports",
        Calls => "calls",
        References => "references",
        Extends => "extends",
        Implements => "implements",
        Overrides => "overrides",
        DependsOn => "depends_on",
        Mentions => "mentions",
        MemberOf => "member_of",
        Annotates => "annotates",
    }
);

string_enum!(VerifiedBy {
    Scip => "scip",
    Lsp => "lsp",
    Syntactic => "syntactic",
    Derived => "derived",
    Note => "note",
});

string_enum!(
    /// Spec §6.5.
    CandidateRole {
        Call => "call",
        Type => "type",
        Import => "import",
        Export => "export",
        Extends => "extends",
        Implements => "implements",
        Value => "value",
    }
);

string_enum!(
    /// Spec §6.3.
    UnresolvedReason {
        NoDefinition => "no_definition",
        ServerUnavailable => "server_unavailable",
        ExternalDependency => "external_dependency",
        UnindexedFile => "unindexed_file",
        Unmapped => "unmapped",
    }
);

string_enum!(FileKind {
    Code => "code",
    Doc => "doc",
    Manifest => "manifest",
    Other => "other",
});

string_enum!(
    /// Why a file was listed but not parsed (spec §7.2, §8.2).
    SkipReason {
        Ignored => "ignored",
        Binary => "binary",
        Oversize => "oversize",
        InvalidUtf8 => "invalid_utf8",
        Unsupported => "unsupported",
        NoRoot => "no_root",
    }
);

string_enum!(NoteKind {
    Note => "note",
    Label => "label",
});

string_enum!(
    /// Where a community's display label came from (spec §6.6).
    LabelSource {
        Folder => "folder",
        Prefix => "prefix",
        Fallback => "fallback",
        Note => "note",
    }
);
