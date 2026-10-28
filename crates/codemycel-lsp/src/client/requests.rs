use std::path::Path;
use std::time::Duration;

use lsp_types::{
    CallHierarchyItem, CallHierarchyOutgoingCall, DocumentSymbolResponse, FileChangeType,
    GotoDefinitionResponse, InitializeResult, Location, Position, Uri,
};
use serde_json::{Value, json};

use super::{ClientError, LspClient, path_to_uri};

impl LspClient {
    /// `initialize` with the root as `rootUri` and its only workspace folder. `server_status`
    /// advertises `experimental/serverStatus`, which quiescence-based readiness waits on.
    pub async fn initialize(
        &self,
        root: &Path,
        initialization_options: Value,
        server_status: bool,
        timeout: Duration,
    ) -> Result<InitializeResult, ClientError> {
        let root_uri = path_to_uri(root)?;
        let name = root
            .file_name()
            .map_or_else(|| "root".to_owned(), |n| n.to_string_lossy().into_owned());
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "codemycel", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": name}],
            "initializationOptions": initialization_options,
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "workspace": {"workspaceFolders": true, "didChangeWatchedFiles": {"dynamicRegistration": false}},
                "textDocument": {
                    "synchronization": {"didSave": false},
                    "definition": {"linkSupport": true},
                    "implementation": {"linkSupport": true},
                    "references": {},
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                    "callHierarchy": {},
                    "publishDiagnostics": {}
                },
                "experimental": {"serverStatusNotification": server_status}
            }
        });
        self.request("initialize", params, timeout).await
    }

    pub fn initialized(&self) -> Result<(), ClientError> {
        self.notify("initialized", json!({}))
    }

    pub fn did_open(&self, uri: &Uri, language_id: &str, text: &str) -> Result<(), ClientError> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": language_id, "version": 1, "text": text}}),
        )
    }

    pub fn did_close(&self, uri: &Uri) -> Result<(), ClientError> {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
    }

    pub fn did_change_watched_files(
        &self,
        changes: &[(Uri, FileChangeType)],
    ) -> Result<(), ClientError> {
        let changes: Vec<Value> = changes
            .iter()
            .map(|(uri, kind)| json!({"uri": uri, "type": kind}))
            .collect();
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": changes }),
        )
    }

    pub async fn definition(
        &self,
        uri: &Uri,
        position: Position,
        timeout: Duration,
    ) -> Result<Vec<Location>, ClientError> {
        let response: Option<GotoDefinitionResponse> = self
            .request(
                "textDocument/definition",
                position_params(uri, position),
                timeout,
            )
            .await?;
        Ok(locations(response))
    }

    pub async fn implementation(
        &self,
        uri: &Uri,
        position: Position,
        timeout: Duration,
    ) -> Result<Vec<Location>, ClientError> {
        let response: Option<GotoDefinitionResponse> = self
            .request(
                "textDocument/implementation",
                position_params(uri, position),
                timeout,
            )
            .await?;
        Ok(locations(response))
    }

    pub async fn references(
        &self,
        uri: &Uri,
        position: Position,
        timeout: Duration,
    ) -> Result<Vec<Location>, ClientError> {
        let mut params = position_params(uri, position);
        params["context"] = json!({"includeDeclaration": false});
        let response: Option<Vec<Location>> = self
            .request("textDocument/references", params, timeout)
            .await?;
        Ok(response.unwrap_or_default())
    }

    pub async fn prepare_call_hierarchy(
        &self,
        uri: &Uri,
        position: Position,
        timeout: Duration,
    ) -> Result<Vec<CallHierarchyItem>, ClientError> {
        let response: Option<Vec<CallHierarchyItem>> = self
            .request(
                "textDocument/prepareCallHierarchy",
                position_params(uri, position),
                timeout,
            )
            .await?;
        Ok(response.unwrap_or_default())
    }

    pub async fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
        timeout: Duration,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, ClientError> {
        let response: Option<Vec<CallHierarchyOutgoingCall>> = self
            .request(
                "callHierarchy/outgoingCalls",
                json!({ "item": item }),
                timeout,
            )
            .await?;
        Ok(response.unwrap_or_default())
    }

    pub async fn document_symbol(
        &self,
        uri: &Uri,
        timeout: Duration,
    ) -> Result<Option<DocumentSymbolResponse>, ClientError> {
        self.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
            timeout,
        )
        .await
    }
}

fn position_params(uri: &Uri, position: Position) -> Value {
    json!({"textDocument": {"uri": uri}, "position": position})
}

/// Links become their target's selection range, the identifier the enricher maps.
fn locations(response: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(GotoDefinitionResponse::Array(locations)) => locations,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location::new(link.target_uri, link.target_selection_range))
            .collect(),
    }
}
