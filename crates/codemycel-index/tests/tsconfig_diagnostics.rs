#![allow(clippy::unwrap_used)]

mod fake;

use std::collections::BTreeMap;
use std::path::Path;

use codemycel_core::config::{Config, HomeConfig, RepoConfig, ServerConfig};
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{Arch, Language, Os, ReadinessStrategy, ServerAdapter, Toolchain};
use codemycel_index::{BatchServers, Pipeline, Registry, Sources, SyncMode};
use codemycel_lsp::adapters::typescript7::TypeScript7;
use codemycel_lsp::{EnrichOptions, Provisioner};
use fake::{FakeAdapter, Repo, fake_lsp};
use serde_json::{Value, json};

/// The fake server standing in for TypeScript 7 on TypeScript roots.
struct FakeTypeScript {
    fake: FakeAdapter,
    real: TypeScript7,
}

impl ServerAdapter for FakeTypeScript {
    fn language(&self) -> Language {
        self.real.language()
    }
    fn name(&self) -> &'static str {
        self.fake.name()
    }
    fn pinned_version(&self) -> &'static str {
        self.fake.pinned_version()
    }
    fn binary_name(&self) -> &'static str {
        self.fake.binary_name()
    }
    fn toolchain(&self) -> Option<Toolchain> {
        None
    }
    fn download_url(&self, _: Os, _: Arch) -> Option<String> {
        None
    }
    fn pinned_sha256(&self, _: Os, _: Arch) -> Option<&'static str> {
        None
    }
    fn archive_member(&self, _: Os) -> Option<&'static str> {
        None
    }
    fn validate(&self, binary: &Path) -> Result<String, String> {
        self.fake.validate(binary)
    }
    fn launch_args(&self) -> &'static [&'static str] {
        self.fake.launch_args()
    }
    fn language_id(&self, extension: &str) -> &'static str {
        self.real.language_id(extension)
    }
    fn initialization_options(&self, config: &ServerConfig) -> Value {
        self.real.initialization_options(config)
    }
    fn readiness(&self) -> ReadinessStrategy {
        self.real.readiness()
    }
    fn request_concurrency(&self) -> usize {
        self.real.request_concurrency()
    }
    fn config_file_name(&self) -> Option<&'static str> {
        self.real.config_file_name()
    }
}

const MAIN: &str = "\
import { helper } from \"./util\";

export function run(): number {
    return helper();
}
";

const UTIL: &str = "export function helper(): number {\n    return 1;\n}\n";

#[test]
fn tsconfig_diagnostics_are_recorded_without_degrading() {
    let repo = Repo::new(&[
        (
            "tsconfig.json",
            "{\n  \"compilerOptions\": {\n    \"baseUrl\": \".\"\n  }\n}\n",
        ),
        ("src/main.ts", MAIN),
        ("src/util.ts", UTIL),
    ]);
    let scenario = repo.scratch.path().join("scenario.json");
    std::fs::write(
        &scenario,
        json!({
            "on_open": [{
                "uri": "src/main.ts",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": "$ROOT/tsconfig.json",
                    "diagnostics": [{
                        "range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 13}},
                        "message": "Option 'baseUrl' has been removed."
                    }]
                }
            }],
            "responses": [{
                "method": "textDocument/definition",
                "result": [repo.location("src/util.ts", "function", "helper")]
            }],
        })
        .to_string(),
    )
    .unwrap();
    let adapter = FakeTypeScript {
        fake: FakeAdapter::new(&scenario),
        real: TypeScript7::new(),
    };
    let mut servers = BTreeMap::new();
    servers.insert(
        Language::TypeScript,
        ServerConfig {
            path: Some(fake_lsp()),
            build_scripts: None,
        },
    );
    let config = Config::merge(
        RepoConfig {
            servers,
            ..RepoConfig::default()
        },
        HomeConfig::default(),
    );
    let provisioner = Provisioner::new(&config, repo.home(), false);
    let registry =
        Registry::default().with_servers(vec![(Language::TypeScript, Box::new(adapter))]);
    let db = tempfile::tempdir().unwrap();
    let mut store = Store::create(&db.path().join("index.db")).unwrap();
    let servers = BatchServers {
        provisioner: &provisioner,
        options: EnrichOptions::default(),
    };
    let sources = Sources {
        servers: &servers,
        scip: None,
        progress: &(),
    };

    Pipeline::sync(
        repo.path(),
        &config,
        &mut store,
        &registry,
        &sources,
        SyncMode::Full,
    )
    .unwrap();

    let roots = store.roots().unwrap();
    let [root] = roots.as_slice() else {
        panic!("expected one root: {roots:?}");
    };
    let status = root.status.as_ref().unwrap();
    assert_eq!(status.degraded, None);
    assert_eq!(
        status.config_diagnostics,
        ["tsconfig.json:3: Option 'baseUrl' has been removed."]
    );
}
