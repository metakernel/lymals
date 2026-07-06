use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use tower_lsp::lsp_types::{
    Location, SymbolInformation, SymbolKind as LspSymbolKind, Url, WorkspaceFolder,
};

use crate::{
    config::LymalsConfig,
    document::DocumentSnapshot,
    parser,
    position::LineIndex,
    semantic::SemanticDocument,
    symbols::{Definition, DefinitionKind},
    syntax::{FileId, ParsedFile, SourceSpan, Symbol},
    workspace::{effective_roots, file_url_to_path, is_workspace_lyma_uri},
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentIndex {
    definitions: Vec<Definition>,
    symbols: Vec<Symbol>,
    by_name: BTreeMap<String, Vec<usize>>,
    by_kind: BTreeMap<DefinitionKind, Vec<usize>>,
}

impl DocumentIndex {
    #[must_use]
    pub fn new(semantic: SemanticDocument) -> Self {
        let mut by_name = BTreeMap::new();
        let mut by_kind = BTreeMap::new();
        let mut symbols = Vec::new();

        for (index, definition) in semantic.definitions.iter().enumerate() {
            by_name
                .entry(definition.name.clone())
                .or_insert_with(Vec::new)
                .push(index);
            by_kind
                .entry(definition.kind)
                .or_insert_with(Vec::new)
                .push(index);
            if let Some(symbol) = definition.as_symbol() {
                symbols.push(symbol);
            }
        }

        Self {
            definitions: semantic.definitions,
            symbols,
            by_name,
            by_kind,
        }
    }

    #[must_use]
    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    #[must_use]
    pub fn by_name(&self, name: &str) -> Vec<&Definition> {
        self.by_name
            .get(name)
            .into_iter()
            .flat_map(|indexes| indexes.iter().map(|index| &self.definitions[*index]))
            .collect()
    }

    #[must_use]
    pub fn by_kind(&self, kind: DefinitionKind) -> Vec<&Definition> {
        self.by_kind
            .get(&kind)
            .into_iter()
            .flat_map(|indexes| indexes.iter().map(|index| &self.definitions[*index]))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDefinition {
    pub uri: Url,
    pub definition: Definition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSymbolMatch {
    pub symbol: SymbolInformation,
    pub score: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedDocument {
    relative_path: Option<PathBuf>,
    text: String,
    index: DocumentIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceIndex {
    documents: BTreeMap<Url, IndexedDocument>,
    invalidations: u64,
}

impl WorkspaceIndex {
    #[must_use]
    pub fn invalidations(&self) -> u64 {
        self.invalidations
    }

    pub fn upsert(&mut self, uri: Url, semantic: SemanticDocument) {
        self.documents.insert(
            uri,
            IndexedDocument {
                relative_path: None,
                text: String::new(),
                index: DocumentIndex::new(semantic),
            },
        );
    }

    pub fn upsert_document(
        &mut self,
        uri: Url,
        text: String,
        semantic: SemanticDocument,
        relative_path: Option<PathBuf>,
    ) {
        self.documents.insert(
            uri,
            IndexedDocument {
                relative_path,
                text,
                index: DocumentIndex::new(semantic),
            },
        );
    }

    pub fn invalidate(&mut self, uri: &Url) -> Option<DocumentIndex> {
        let removed = self.documents.remove(uri);
        if removed.is_some() {
            self.invalidations = self.invalidations.saturating_add(1);
        }
        removed.map(|document| document.index)
    }

    pub fn invalidate_many<'a>(&mut self, uris: impl IntoIterator<Item = &'a Url>) -> usize {
        let mut removed = 0;
        for uri in uris {
            if self.invalidate(uri).is_some() {
                removed += 1;
            }
        }
        removed
    }

    #[must_use]
    pub fn document(&self, uri: &Url) -> Option<&DocumentIndex> {
        self.documents.get(uri).map(|document| &document.index)
    }

    #[must_use]
    pub fn documents(&self) -> Vec<(&Url, &DocumentIndex)> {
        self.documents
            .iter()
            .map(|(uri, document)| (uri, &document.index))
            .collect()
    }

    #[must_use]
    pub fn load(
        open_documents: &[DocumentSnapshot],
        workspace_folders: &[WorkspaceFolder],
        config: &LymalsConfig,
    ) -> Self {
        let mut index = Self::default();
        let mut seen = BTreeSet::new();

        for document in open_documents {
            if seen.insert(document.uri.clone()) {
                index.insert_snapshot(
                    document.uri.clone(),
                    document.file_id,
                    document.text.clone(),
                    workspace_folders,
                    config,
                );
            }
        }

        if config.index_workspace {
            let open_paths = seen
                .iter()
                .filter_map(file_url_to_path)
                .collect::<BTreeSet<_>>();
            let mut next_file_id = open_documents
                .iter()
                .map(|document| document.file_id.0)
                .max()
                .unwrap_or_default()
                .saturating_add(1);

            for root in effective_roots(workspace_folders, config) {
                let mut indexed = 0u32;
                walk_workspace(root.as_path(), &mut |path| {
                    if indexed >= config.max_indexed_files_per_workspace
                        || open_paths.contains(path)
                    {
                        return;
                    }
                    let Ok(uri) = Url::from_file_path(path) else {
                        return;
                    };
                    if !seen.insert(uri.clone())
                        || !is_workspace_lyma_uri(&uri, workspace_folders, config)
                    {
                        return;
                    }
                    let Ok(metadata) = fs::metadata(path) else {
                        return;
                    };
                    if metadata.len() > u64::from(config.max_indexed_file_bytes) {
                        return;
                    }
                    let Ok(text) = fs::read_to_string(path) else {
                        return;
                    };
                    index.insert_snapshot(
                        uri,
                        FileId(next_file_id),
                        text,
                        workspace_folders,
                        config,
                    );
                    next_file_id = next_file_id.saturating_add(1);
                    indexed = indexed.saturating_add(1);
                });
            }
        }

        index
    }

    #[must_use]
    pub fn workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let normalized_query = normalize_query(query);
        let mut matches = self.query_matches(&normalized_query);
        matches.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.symbol.name.cmp(&right.symbol.name))
                .then_with(|| left.symbol.location.uri.cmp(&right.symbol.location.uri))
                .then_with(|| {
                    left.symbol
                        .location
                        .range
                        .start
                        .line
                        .cmp(&right.symbol.location.range.start.line)
                })
                .then_with(|| {
                    left.symbol
                        .location
                        .range
                        .start
                        .character
                        .cmp(&right.symbol.location.range.start.character)
                })
        });
        matches.into_iter().map(|item| item.symbol).collect()
    }

    fn query_matches(&self, normalized_query: &str) -> Vec<WorkspaceSymbolMatch> {
        let mut matches = Vec::new();

        for (uri, document) in &self.documents {
            let relative_path = document
                .relative_path
                .as_deref()
                .map(normalize_path_for_display)
                .unwrap_or_default();

            for definition in document.index.definitions() {
                if let Some(symbol_match) = workspace_symbol_from_definition(
                    uri,
                    document,
                    definition,
                    &relative_path,
                    normalized_query,
                ) {
                    matches.push(symbol_match);
                }
            }
        }

        matches
    }

    fn insert_snapshot(
        &mut self,
        uri: Url,
        file_id: FileId,
        text: String,
        workspace_folders: &[WorkspaceFolder],
        config: &LymalsConfig,
    ) {
        let parsed = parser::parse_fallback(file_id, uri.as_str(), &text);
        let file = match parsed.file {
            ParsedFile::Fallback(file) => file,
            #[cfg(feature = "upstream-lyma")]
            ParsedFile::Upstream(_) => return,
        };
        let semantic = SemanticDocument::from_ast(&file.ast);
        let relative_path = file_url_to_path(&uri)
            .and_then(|path| relative_path_for_workspace(&path, workspace_folders, config));
        self.upsert_document(uri, text, semantic, relative_path);
    }
    #[must_use]
    pub fn definitions_named(&self, name: &str) -> Vec<WorkspaceDefinition> {
        let mut matches = Vec::new();
        for (uri, document) in &self.documents {
            for definition in document.index.by_name(name) {
                matches.push(WorkspaceDefinition {
                    uri: uri.clone(),
                    definition: definition.clone(),
                });
            }
        }
        matches
    }
}

fn workspace_symbol_from_definition(
    uri: &Url,
    document: &IndexedDocument,
    definition: &Definition,
    relative_path: &str,
    normalized_query: &str,
) -> Option<WorkspaceSymbolMatch> {
    let candidate = symbol_candidate(definition)?;
    let searchable = vec![
        candidate.name.clone(),
        candidate.kind_label.to_string(),
        relative_path.to_string(),
        definition.path.to_string(),
        definition.detail.clone().unwrap_or_default(),
    ];
    let score = score_match(normalized_query, &searchable)?;
    let range = symbol_span(definition, &document.text).and_then(|span| {
        LineIndex::new(&document.text)
            .span_to_range(&document.text, span)
            .ok()
    })?;

    let container_name = candidate.container_name(relative_path, definition);

    #[allow(deprecated)]
    Some(WorkspaceSymbolMatch {
        symbol: SymbolInformation {
            name: candidate.name,
            kind: candidate.kind,
            tags: None,
            deprecated: None,
            location: Location {
                uri: uri.clone(),
                range,
            },
            container_name,
        },
        score,
    })
}

struct SymbolCandidate {
    name: String,
    kind: LspSymbolKind,
    kind_label: &'static str,
}

impl SymbolCandidate {
    fn container_name(&self, relative_path: &str, definition: &Definition) -> Option<String> {
        let scope = match definition.kind {
            DefinitionKind::DocumentRoot => relative_path.to_string(),
            DefinitionKind::Import => {
                if relative_path.is_empty() {
                    "path".to_string()
                } else {
                    format!("{relative_path} • path")
                }
            }
            _ => {
                if relative_path.is_empty() {
                    definition.path.to_string()
                } else {
                    format!("{relative_path} • {}", definition.path)
                }
            }
        };
        (!scope.is_empty()).then_some(scope)
    }
}

fn symbol_candidate(definition: &Definition) -> Option<SymbolCandidate> {
    match definition.kind {
        DefinitionKind::DocumentRoot => Some(SymbolCandidate {
            name: definition.name.clone(),
            kind: LspSymbolKind::MODULE,
            kind_label: "document",
        }),
        DefinitionKind::Import => Some(SymbolCandidate {
            name: definition.name.clone(),
            kind: LspSymbolKind::FILE,
            kind_label: "path",
        }),
        DefinitionKind::Let => Some(SymbolCandidate {
            name: definition.name.clone(),
            kind: LspSymbolKind::VARIABLE,
            kind_label: "let",
        }),
        DefinitionKind::Key => Some(SymbolCandidate {
            name: definition.name.clone(),
            kind: LspSymbolKind::KEY,
            kind_label: "key",
        }),
        DefinitionKind::Tag => Some(SymbolCandidate {
            name: format!("!{}", definition.name),
            kind: LspSymbolKind::OPERATOR,
            kind_label: "tag",
        }),
        DefinitionKind::Directive if definition.name == "@schema" => Some(SymbolCandidate {
            name: definition
                .detail
                .clone()
                .unwrap_or_else(|| "@schema".to_string())
                .trim()
                .trim_matches(['\'', '"'])
                .to_string(),
            kind: LspSymbolKind::CLASS,
            kind_label: "schema",
        }),
        _ => None,
    }
}

fn symbol_span(definition: &Definition, text: &str) -> Option<SourceSpan> {
    match definition.kind {
        DefinitionKind::Directive if definition.name == "@schema" => {
            let value = definition.detail.as_deref()?.trim();
            let line = text.get(definition.span.start..definition.span.end)?;
            let local_start = line.find(value)?;
            Some(SourceSpan::new(
                definition.span.file_id,
                definition.span.start + local_start,
                definition.span.start + local_start + value.len(),
            ))
        }
        DefinitionKind::Import => {
            let line = text.get(definition.span.start..definition.span.end)?;
            let local_start = line.find(&definition.name)?;
            Some(SourceSpan::new(
                definition.span.file_id,
                definition.span.start + local_start,
                definition.span.start + local_start + definition.name.len(),
            ))
        }
        DefinitionKind::Let => Some(SourceSpan::new(
            definition.span.file_id,
            definition.span.start + 4,
            definition.span.start + 4 + definition.name.len(),
        )),
        DefinitionKind::Tag => {
            let line = text.get(definition.span.start..definition.span.end)?;
            let needle = format!("!{}", definition.name);
            let local_start = line.find(&needle)? + 1;
            Some(SourceSpan::new(
                definition.span.file_id,
                definition.span.start + local_start,
                definition.span.start + local_start + definition.name.len(),
            ))
        }
        _ => Some(definition.selection_span),
    }
}

fn score_match(query: &str, searchable: &[String]) -> Option<u32> {
    if query.is_empty() {
        return Some(1);
    }

    searchable
        .iter()
        .filter_map(|candidate| score_candidate(query, candidate))
        .max()
}

fn score_candidate(query: &str, candidate: &str) -> Option<u32> {
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }

    let lower = candidate.to_ascii_lowercase();
    if let Some(index) = lower.find(query) {
        return Some(10_000u32.saturating_sub(index as u32));
    }

    fuzzy_subsequence_score(query, &lower)
}

fn fuzzy_subsequence_score(query: &str, candidate: &str) -> Option<u32> {
    let mut query_chars = query.chars();
    let mut current = query_chars.next()?;
    let mut score = 0u32;

    for (index, ch) in candidate.chars().enumerate() {
        if ch == current {
            score = score
                .saturating_add(100)
                .saturating_add(50u32.saturating_sub(index as u32));
            if let Some(next) = query_chars.next() {
                current = next;
            } else {
                return Some(score);
            }
        }
    }

    None
}

fn normalize_query(query: &str) -> String {
    query.trim().to_ascii_lowercase()
}

fn walk_workspace(root: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_workspace(&path, visit);
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "lyma") {
            visit(&path);
        }
    }
}

fn relative_path_for_workspace(
    path: &Path,
    workspace_folders: &[WorkspaceFolder],
    config: &LymalsConfig,
) -> Option<PathBuf> {
    effective_roots(workspace_folders, config)
        .into_iter()
        .find_map(|root| path.strip_prefix(root).ok().map(Path::to_path_buf))
}

fn normalize_path_for_display(path: &Path) -> String {
    path.iter()
        .map(|segment| segment.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
