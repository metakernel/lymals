use std::{collections::BTreeSet, fs, path::Path};

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, InsertTextFormat, Position, Url,
    WorkspaceFolder,
};

use crate::{
    config::LumalsConfig,
    imports::resolve_guarded_import,
    parser,
    semantic::SemanticDocument,
    symbols::DefinitionKind,
    workspace::{effective_roots, file_url_to_path, is_workspace_luma_uri},
};

const DIRECTIVES: &[DirectiveSpec] = &[
    DirectiveSpec::new(
        "@import",
        "Import a local module",
        "@import \"${1:./shared.luma}\" as ${2:shared}",
    ),
    DirectiveSpec::new(
        "@include",
        "Include a local document",
        "@include \"${1:./partials/base.luma}\"",
    ),
    DirectiveSpec::new(
        "@use",
        "Use a local module alias",
        "@use \"${1:./modules/network.luma}\" as ${2:network}",
    ),
    DirectiveSpec::new("@profile", "Select a profile", "@profile ${1:dev}"),
    DirectiveSpec::new("@luma", "Declare the Luma version", "@luma ${1:1}"),
];

const PROFILE_VALUES: &[&str] = &[
    "dev",
    "development",
    "test",
    "staging",
    "prod",
    "production",
];

const SCALAR_VALUES: &[(&str, &str)] = &[
    ("true", "Boolean true"),
    ("false", "Boolean false"),
    ("null", "Null value"),
    ("nil", "Lua nil"),
];

const LUA_ENV_NAMES: &[(&str, &str)] = &[
    ("env", "Environment bindings"),
    ("profile", "Active profile"),
    ("vars", "Template variables"),
    ("module", "Current module"),
    ("import", "Imported values"),
];

#[derive(Debug, Clone)]
pub struct CompletionRequest<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub file_id: crate::syntax::FileId,
    pub position: Position,
    pub workspace_folders: &'a [WorkspaceFolder],
    pub config: &'a LumalsConfig,
}

pub fn complete(request: CompletionRequest<'_>) -> Option<CompletionResponse> {
    if !request.config.completion.enabled {
        return None;
    }

    let offset = position_to_offset(request.text, request.position)?;
    let line_range = line_bounds(request.text, offset);
    let line_prefix = &request.text[line_range.start..offset];
    let trimmed_prefix = line_prefix.trim_start();

    if trimmed_prefix.starts_with('#') {
        return None;
    }

    let context = CompletionCorpus::load(
        request.uri,
        request.text,
        request.file_id,
        request.workspace_folders,
        request.config,
    );

    let items = if let Some(items) = directive_name_items(trimmed_prefix) {
        items
    } else if let Some(items) = directive_argument_items(trimmed_prefix, &context, request) {
        items
    } else if is_lua_expression_context(line_prefix) {
        lua_items(current_word(line_prefix), &context)
    } else if let Some(items) = key_items(line_prefix, &context) {
        items
    } else if is_value_context(line_prefix) {
        scalar_value_items(current_word(line_prefix))
    } else {
        top_level_snippets(current_word(line_prefix))
    };

    (!items.is_empty()).then_some(CompletionResponse::Array(items))
}

#[derive(Debug, Clone, Copy)]
struct DirectiveSpec {
    name: &'static str,
    detail: &'static str,
    snippet: &'static str,
}

impl DirectiveSpec {
    const fn new(name: &'static str, detail: &'static str, snippet: &'static str) -> Self {
        Self {
            name,
            detail,
            snippet,
        }
    }
}

#[derive(Debug, Default)]
struct CompletionCorpus {
    keys: BTreeSet<String>,
    lets: BTreeSet<String>,
    aliases: BTreeSet<String>,
}

impl CompletionCorpus {
    fn load(
        current_uri: &Url,
        current_text: &str,
        file_id: crate::syntax::FileId,
        workspace_folders: &[WorkspaceFolder],
        config: &LumalsConfig,
    ) -> Self {
        let mut corpus = Self::default();
        corpus.ingest(current_uri.as_str(), current_text, file_id);

        if !config.index_workspace {
            return corpus;
        }

        let current_path = file_url_to_path(current_uri);
        let mut seen = BTreeSet::new();
        let mut indexed = 0u32;

        for root in effective_roots(workspace_folders, config) {
            walk_luma_files(&root, &mut |path| {
                if indexed >= config.max_indexed_files_per_workspace {
                    return;
                }
                if current_path.as_deref() == Some(path) || !seen.insert(path.to_path_buf()) {
                    return;
                }
                let Ok(uri) = Url::from_file_path(path) else {
                    return;
                };
                if !is_workspace_luma_uri(&uri, workspace_folders, config) {
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
                indexed = indexed.saturating_add(1);
                corpus.ingest(uri.as_str(), &text, crate::syntax::FileId(indexed));
            });
        }

        corpus
    }

    fn ingest(&mut self, name: &str, text: &str, file_id: crate::syntax::FileId) {
        let parsed = parser::parse_fallback(file_id, name, text);
        let file = match parsed.file {
            crate::syntax::ParsedFile::Fallback(file) => file,
            #[cfg(feature = "upstream-luma")]
            crate::syntax::ParsedFile::Upstream(_) => return,
        };
        let semantic = SemanticDocument::from_ast(&file.ast);
        for definition in semantic.definitions {
            match definition.kind {
                DefinitionKind::Key => {
                    self.keys.insert(definition.name);
                }
                DefinitionKind::Let => {
                    self.lets.insert(definition.name);
                }
                DefinitionKind::Alias => {
                    self.aliases.insert(definition.name);
                }
                _ => {}
            }
        }
    }
}

fn directive_name_items(trimmed_prefix: &str) -> Option<Vec<CompletionItem>> {
    if !trimmed_prefix.starts_with('@') || trimmed_prefix.contains(char::is_whitespace) {
        return None;
    }

    let prefix = trimmed_prefix;
    Some(
        DIRECTIVES
            .iter()
            .filter(|directive| directive.name.starts_with(prefix))
            .map(|directive| CompletionItem {
                label: directive.name.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(directive.detail.to_string()),
                insert_text: Some(directive.snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                commit_characters: Some(vec![" ".to_string()]),
                ..CompletionItem::default()
            })
            .collect(),
    )
}

fn directive_argument_items(
    trimmed_prefix: &str,
    context: &CompletionCorpus,
    request: CompletionRequest<'_>,
) -> Option<Vec<CompletionItem>> {
    if !trimmed_prefix.starts_with('@') {
        return None;
    }

    let mut parts = trimmed_prefix.splitn(2, char::is_whitespace);
    let directive = parts.next()?;
    let remainder = parts.next()?.trim_start();

    match directive {
        "@profile" => Some(
            PROFILE_VALUES
                .iter()
                .filter(|value| value.starts_with(remainder))
                .map(|value| CompletionItem {
                    label: (*value).to_string(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: Some("Profile value".to_string()),
                    insert_text: Some(value.trim_start_matches(remainder).to_string()),
                    commit_characters: Some(vec!["\n".to_string()]),
                    ..CompletionItem::default()
                })
                .collect(),
        ),
        "@import" | "@include" | "@use" => {
            if let Some(alias_prefix) = remainder.rsplit_once(" as ").map(|(_, alias)| alias) {
                Some(alias_items(alias_prefix, remainder, context))
            } else {
                Some(path_items(directive, remainder, request))
            }
        }
        _ => None,
    }
}

fn alias_items(prefix: &str, remainder: &str, context: &CompletionCorpus) -> Vec<CompletionItem> {
    let mut aliases = BTreeSet::new();
    if let Some(target) = remainder
        .split_once(" as ")
        .map(|(target, _)| target.trim())
    {
        if let Some(stem) = alias_from_target(target) {
            aliases.insert(stem);
        }
    }
    aliases.extend(context.aliases.iter().cloned());

    aliases
        .into_iter()
        .filter(|alias| alias.starts_with(prefix))
        .map(|alias| CompletionItem {
            label: alias.clone(),
            kind: Some(CompletionItemKind::MODULE),
            insert_text: Some(alias.trim_start_matches(prefix).to_string()),
            commit_characters: Some(vec!["\n".to_string()]),
            ..CompletionItem::default()
        })
        .collect()
}

fn path_items(
    directive: &str,
    remainder: &str,
    request: CompletionRequest<'_>,
) -> Vec<CompletionItem> {
    let path_prefix = trim_quotes(remainder);
    let current_path = match file_url_to_path(request.uri) {
        Some(path) => path,
        None => return Vec::new(),
    };
    let base_dir = match current_path.parent() {
        Some(path) => path,
        None => return Vec::new(),
    };

    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in
        collect_relative_luma_paths(base_dir, request.workspace_folders, request.config)
    {
        if !candidate.starts_with(path_prefix) || !seen.insert(candidate.clone()) {
            continue;
        }
        if resolve_guarded_import(
            request.uri,
            &candidate,
            request.workspace_folders,
            request.config,
        )
        .is_err()
        {
            continue;
        }

        items.push(CompletionItem {
            label: candidate.clone(),
            kind: Some(CompletionItemKind::FILE),
            detail: Some(format!("{} path", directive.trim_start_matches('@'))),
            insert_text: Some(candidate.trim_start_matches(path_prefix).to_string()),
            commit_characters: Some(vec!["\"".to_string(), "'".to_string(), " ".to_string()]),
            ..CompletionItem::default()
        });
    }

    items
}

fn lua_items(prefix: &str, context: &CompletionCorpus) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (name, detail) in LUA_ENV_NAMES {
        if name.starts_with(prefix) {
            items.push(CompletionItem {
                label: (*name).to_string(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some((*detail).to_string()),
                insert_text: Some(name.trim_start_matches(prefix).to_string()),
                ..CompletionItem::default()
            });
        }
    }

    for name in &context.lets {
        if name.starts_with(prefix) {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some("Local let binding".to_string()),
                insert_text: Some(name.trim_start_matches(prefix).to_string()),
                ..CompletionItem::default()
            });
        }
    }

    for name in &context.aliases {
        if name.starts_with(prefix) {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some("Imported/module alias".to_string()),
                insert_text: Some(name.trim_start_matches(prefix).to_string()),
                ..CompletionItem::default()
            });
        }
    }

    items.extend(scalar_value_items(prefix));

    items
}

fn key_items(line_prefix: &str, context: &CompletionCorpus) -> Option<Vec<CompletionItem>> {
    if line_prefix.contains(':') || line_prefix.trim_start().starts_with(['@', '#', '-']) {
        return None;
    }

    let prefix = current_word(line_prefix);
    let mut items: Vec<_> = context
        .keys
        .iter()
        .filter(|key| key.starts_with(prefix))
        .map(|key| CompletionItem {
            label: key.clone(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("Known mapping key".to_string()),
            insert_text: Some(format!("{}: ", key.trim_start_matches(prefix))),
            commit_characters: Some(vec![": ".trim().to_string()]),
            ..CompletionItem::default()
        })
        .collect();

    if prefix.is_empty() || "let binding".starts_with(prefix) {
        items.push(CompletionItem {
            label: "let binding".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("let ${1:name} = ${2:value}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("Create a let binding".to_string()),
            ..CompletionItem::default()
        });
    }

    Some(items)
}

fn scalar_value_items(prefix: &str) -> Vec<CompletionItem> {
    SCALAR_VALUES
        .iter()
        .filter(|(value, _)| value.starts_with(prefix))
        .map(|(value, detail)| CompletionItem {
            label: (*value).to_string(),
            kind: Some(CompletionItemKind::VALUE),
            detail: Some((*detail).to_string()),
            insert_text: Some(value.trim_start_matches(prefix).to_string()),
            commit_characters: Some(vec!["\n".to_string()]),
            ..CompletionItem::default()
        })
        .collect()
}

fn top_level_snippets(prefix: &str) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    if prefix.is_empty() || "mapping entry".starts_with(prefix) {
        items.push(CompletionItem {
            label: "mapping entry".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("${1:key}: ${2:value}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("Create a mapping entry".to_string()),
            ..CompletionItem::default()
        });
    }
    items
}

fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let target_line = usize::try_from(position.line).ok()?;
    let target_col = usize::try_from(position.character).ok()?;
    let mut line = 0usize;
    let mut line_start = 0usize;

    for segment in text.split_inclusive('\n') {
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        if line == target_line {
            return Some(line_start + target_col.min(body.len()));
        }
        line += 1;
        line_start += segment.len();
    }

    (line == target_line).then_some(text.len())
}

fn line_bounds(text: &str, offset: usize) -> std::ops::Range<usize> {
    let start = text[..offset].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = text[offset..]
        .find('\n')
        .map(|idx| offset + idx)
        .unwrap_or(text.len());
    start..end
}

fn current_word(prefix: &str) -> &str {
    prefix
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
        .next()
        .unwrap_or_default()
}

fn is_value_context(line_prefix: &str) -> bool {
    line_prefix.contains(':')
}

fn is_lua_expression_context(line_prefix: &str) -> bool {
    line_prefix
        .rfind("${")
        .is_some_and(|open| line_prefix[open + 2..].find('}').is_none())
        || line_prefix
            .trim_start()
            .strip_prefix("let ")
            .and_then(|rest| rest.split_once('='))
            .is_some()
        || line_prefix.trim_end().ends_with('=')
}

fn trim_quotes(value: &str) -> &str {
    value.trim().trim_matches(['"', '\''])
}

fn alias_from_target(target: &str) -> Option<String> {
    let target = trim_quotes(target);
    Path::new(target)
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(['-', '.'], "_"))
        .filter(|stem| !stem.is_empty())
}

fn collect_relative_luma_paths(
    base_dir: &Path,
    workspace_folders: &[WorkspaceFolder],
    config: &LumalsConfig,
) -> Vec<String> {
    let mut out = Vec::new();
    let roots = effective_roots(workspace_folders, config);

    for root in roots {
        walk_luma_files(&root, &mut |path| {
            if let Ok(relative) = path.strip_prefix(base_dir) {
                let rendered = render_relative_path(relative);
                if !rendered.is_empty() {
                    out.push(rendered);
                }
            }
        });
    }

    out.sort();
    out
}

fn render_relative_path(path: &Path) -> String {
    let value = path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");

    if value.is_empty() {
        value
    } else {
        format!("./{value}")
    }
}

fn walk_luma_files(root: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_luma_files(&path, visit);
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "luma") {
            visit(&path);
        }
    }
}
