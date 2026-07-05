use crate::{
    ast::{
        AstFile, Directive, DocumentItem, LetBinding, MappingEntry, Node, Scalar, ScalarKind,
        Sequence, TagNode,
    },
    diagnostics::{self, Diagnostic},
    parser,
    semantic::SemanticDocument,
    symbols::{Definition, DefinitionKind, PathSegment, SymbolPath},
    syntax::{FileId, ParsedFile, SourceSpan},
};

#[derive(Debug, Clone, Copy)]
pub struct HoverRequest<'a> {
    pub uri: &'a str,
    pub text: &'a str,
    pub file_id: FileId,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    pub span: SourceSpan,
    pub markdown: String,
}

pub fn hover(request: HoverRequest<'_>) -> Option<HoverResult> {
    let parsed = parser::parse_fallback(request.file_id, request.uri, request.text);
    let source = parsed.source.clone();
    let diagnostics = diagnostics::collect(&parsed);
    let file = match &parsed.file {
        ParsedFile::Fallback(file) => file,
        #[cfg(feature = "upstream-luma")]
        ParsedFile::Upstream(_) => return None,
    };
    let semantic = SemanticDocument::from_ast(&file.ast);
    let context = HoverContext {
        source: &source,
        definitions: &semantic.definitions,
        diagnostics: &diagnostics,
    };

    let mut hover = find_file_hover(&file.ast, request.offset, &context)
        .or_else(|| diagnostic_hover(request.offset, &context));

    if let Some(hover) = hover.as_mut() {
        append_diagnostics(hover, request.offset, &context);
    }

    hover
}

struct HoverContext<'a> {
    source: &'a crate::syntax::SourceText,
    definitions: &'a [Definition],
    diagnostics: &'a [Diagnostic],
}

fn find_file_hover(
    file: &AstFile,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    for (document_index, document) in file.documents.iter().enumerate() {
        let path = SymbolPath::root(document_index + 1);
        for item in &document.items {
            let hover = match item {
                DocumentItem::Directive(directive) => {
                    directive_hover(directive, &path, offset, context)
                }
                DocumentItem::Let(binding) => let_hover(binding, offset, context),
                DocumentItem::Node(node) => node_hover(node, &path, offset, context),
                DocumentItem::Comment(_) => None,
            };

            if hover.is_some() {
                return hover;
            }
        }
    }

    None
}

fn directive_hover(
    directive: &Directive,
    path: &SymbolPath,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    let line = context.source.slice(directive.span)?;
    let trimmed_start = line.len().saturating_sub(line.trim_start().len());
    let name_len = directive.name.len();
    let name_span = SourceSpan::new(
        directive.span.file_id,
        directive.span.start + trimmed_start,
        directive.span.start + trimmed_start + name_len,
    );

    if contains(name_span, offset) {
        let spec = directive_spec(&directive.name)?;
        return Some(HoverResult {
            span: name_span,
            markdown: format!(
                "**Directive:** `{}`\n\n{}\n\n```luma\n{}\n```\n\nSpec excerpt: {}\n\n**Static node type:** `directive`",
                spec.name, spec.summary, spec.example, spec.excerpt,
            ),
        });
    }

    let alias_span = directive_alias_span(directive, line)?;
    if contains(alias_span, offset) {
        let alias = context.source.slice(alias_span)?;
        let kind = if directive.name == "@use" {
            "module"
        } else {
            "import"
        };
        return Some(HoverResult {
            span: alias_span,
            markdown: format!(
                "**Alias:** `{alias}`\n\nImported with `{}`.\nTarget: `{}`\n\nAvailable inside parse-only Lua expressions as a static {kind} alias.\n\n**Static node type:** `alias`",
                directive.name,
                directive_target(directive).unwrap_or_default(),
            ),
        });
    }

    let _ = path;
    None
}

fn let_hover(
    binding: &LetBinding,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    let name_span = let_name_span(binding)?;
    if contains(name_span, offset) {
        return Some(HoverResult {
            span: name_span,
            markdown: format!(
                "**Let binding:** `{}`\n\nLocal parse-only binding available to `${{...}}` expressions in the same document.\n\n**Static node type:** `binding`",
                binding.name
            ),
        });
    }

    let value_span = binding.value_span?;
    let value = trimmed_value_span(context.source, value_span)?;
    if !contains(value, offset) {
        return None;
    }

    value_reference_hover(value, offset, context)
}

fn node_hover(
    node: &Node,
    path: &SymbolPath,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    match node {
        Node::Mapping(mapping) => {
            for entry in &mapping.entries {
                if contains(entry.key_span, offset) {
                    return Some(key_hover(
                        entry,
                        key_path(context.source, entry).unwrap_or_else(|| {
                            path.child(PathSegment::Key(entry.key.clone())).to_string()
                        }),
                    ));
                }
                if let Some(value) = &entry.value {
                    if let Some(hover) = node_hover(
                        value,
                        &path.child(PathSegment::Key(entry.key.clone())),
                        offset,
                        context,
                    ) {
                        return Some(hover);
                    }
                }
            }
            None
        }
        Node::Sequence(Sequence { items, .. }) => {
            for (index, item) in items.iter().enumerate() {
                if let Some(value) = &item.value {
                    if let Some(hover) = node_hover(
                        value,
                        &path.child(PathSegment::Index(index)),
                        offset,
                        context,
                    ) {
                        return Some(hover);
                    }
                }
            }
            None
        }
        Node::Scalar(scalar) => scalar_hover(scalar, path, offset, context),
        Node::Tag(TagNode { name, span, value }) => {
            if contains(*span, offset) {
                return Some(HoverResult {
                    span: *span,
                    markdown: format!(
                        "**Tag:** `!{name}`\n\nApplies a tagged interpretation to the following static node.\n\n**Static node type:** `tag`"
                    ),
                });
            }
            value
                .as_deref()
                .and_then(|value| node_hover(value, path, offset, context))
        }
        Node::Spread(_) | Node::Conditional(_) | Node::Loop(_) | Node::Error(_) => None,
    }
}

fn scalar_hover(
    scalar: &Scalar,
    _path: &SymbolPath,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    if !contains(scalar.span, offset) {
        return None;
    }

    if scalar.kind == ScalarKind::LuaExpression
        && let Some((span, ident)) = lua_identifier_hover(scalar, offset, context)
    {
        let alias = context.definitions.iter().find(|definition| {
            definition.kind == DefinitionKind::Alias && definition.name == ident
        });
        if let Some(alias) = alias {
            return Some(HoverResult {
                span,
                markdown: format!(
                    "**Alias:** `{}`\n\nImported/module alias.\nSource path: `{}`\nPath: `{}`\n\nAvailable inside parse-only Lua expressions without evaluation.\n\n**Static node type:** `alias`",
                    alias.name,
                    alias.detail.as_deref().unwrap_or("unknown"),
                    alias.path,
                ),
            });
        }

        let binding = context
            .definitions
            .iter()
            .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident);
        if let Some(binding) = binding {
            return Some(HoverResult {
                span,
                markdown: format!(
                    "**Let binding:** `{}`\n\nResolved from the local semantic index.\nPath: `{}`\n\n**Static node type:** `binding`",
                    binding.name, binding.path,
                ),
            });
        }
    }

    let scalar_type = scalar_type_name(scalar);
    Some(HoverResult {
        span: scalar.span,
        markdown: format!(
            "**Scalar:** `{}`\n\nLiteral: `{}`\nStatic node type: `{}`\n\nSpec excerpt: Plain scalars stay static in parse-only mode; hover never evaluates Lua to refine runtime values.",
            scalar_type,
            escape_inline_code(&scalar.text),
            scalar_type,
        ),
    })
}

fn key_hover(entry: &MappingEntry, path: String) -> HoverResult {
    HoverResult {
        span: entry.key_span,
        markdown: format!(
            "**Key:** `{}`\n\nPath: `{}`\nStatic value type: `{}`\n\nSpec excerpt: Mapping keys select child nodes using `key: value` syntax and indentation-delimited blocks.",
            entry.key,
            path,
            entry.value.as_deref().map(node_type_name).unwrap_or("null"),
        ),
    }
}

fn diagnostic_hover(offset: usize, context: &HoverContext<'_>) -> Option<HoverResult> {
    let diagnostic = context.diagnostics.iter().find(|diagnostic| {
        diagnostic.primary_span.is_some_and(|span| {
            diagnostics::stable_span(context.source, span)
                .is_some_and(|span| contains(span, offset))
        })
    })?;
    let span = diagnostics::stable_span(context.source, diagnostic.primary_span?)?;
    let single = [diagnostic];
    Some(HoverResult {
        span,
        markdown: render_diagnostics_section(&single),
    })
}

fn append_diagnostics(hover: &mut HoverResult, offset: usize, context: &HoverContext<'_>) {
    let matching = context
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.primary_span.is_some_and(|span| {
                diagnostics::stable_span(context.source, span)
                    .is_some_and(|span| contains(span, offset))
            })
        })
        .collect::<Vec<_>>();

    if matching.is_empty() {
        return;
    }

    let section = render_diagnostics_section(&matching);
    if hover.markdown == section {
        return;
    }
    hover.markdown.push_str("\n\n");
    hover.markdown.push_str(&section);
}

fn render_diagnostics_section(diagnostics: &[&Diagnostic]) -> String {
    let mut out = String::from("**Diagnostics**");
    for diagnostic in diagnostics {
        out.push_str(&format!("\n- `{}` {}", diagnostic.code, diagnostic.message));
        for note in &diagnostic.notes {
            out.push_str(&format!("\n  - {}", note));
        }
    }
    out
}

fn directive_alias_span(directive: &Directive, line: &str) -> Option<SourceSpan> {
    if !matches!(directive.name.as_str(), "@import" | "@use") {
        return None;
    }

    let alias = directive.value.as_ref()?.rsplit_once(" as ")?.1.trim();
    if alias.is_empty() {
        return None;
    }
    let local_start = line.rfind(alias)?;
    Some(SourceSpan::new(
        directive.span.file_id,
        directive.span.start + local_start,
        directive.span.start + local_start + alias.len(),
    ))
}

fn directive_target(directive: &Directive) -> Option<String> {
    let value = directive.value.as_ref()?.trim();
    let target = value
        .split_once(" as ")
        .map(|(target, _)| target)
        .unwrap_or(value);
    Some(strip_quotes(target).to_owned())
}

fn let_name_span(binding: &LetBinding) -> Option<SourceSpan> {
    let start = binding.span.start;
    let line_len = binding.span.len();
    let prefix = 4usize;
    let name_start = start + prefix;
    let name_end = name_start + binding.name.len();
    (binding.span.end >= name_end && line_len >= prefix).then_some(SourceSpan::new(
        binding.span.file_id,
        name_start,
        name_end,
    ))
}

fn lua_identifier_hover(
    scalar: &Scalar,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<(SourceSpan, String)> {
    identifier_at_span(scalar.span, offset, context)
}

fn value_reference_hover(
    span: SourceSpan,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<HoverResult> {
    let (span, ident) = identifier_at_span(span, offset, context)?;
    let alias = context
        .definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Alias && definition.name == ident);
    if let Some(alias) = alias {
        return Some(HoverResult {
            span,
            markdown: format!(
                "**Alias:** `{}`\n\nImported/module alias.\nSource path: `{}`\nPath: `{}`\n\nAvailable inside parse-only Lua expressions without evaluation.\n\n**Static node type:** `alias`",
                alias.name,
                alias.detail.as_deref().unwrap_or("unknown"),
                alias.path,
            ),
        });
    }

    let binding = context
        .definitions
        .iter()
        .find(|definition| definition.kind == DefinitionKind::Let && definition.name == ident);
    if let Some(binding) = binding {
        return Some(HoverResult {
            span,
            markdown: format!(
                "**Let binding:** `{}`\n\nResolved from the local semantic index.\nPath: `{}`\n\n**Static node type:** `binding`",
                binding.name, binding.path,
            ),
        });
    }

    None
}

fn identifier_at_span(
    span: SourceSpan,
    offset: usize,
    context: &HoverContext<'_>,
) -> Option<(SourceSpan, String)> {
    let text = context.source.slice(span)?;
    let relative = offset.checked_sub(span.start)?;
    let bytes = text.as_bytes();
    if relative >= bytes.len() {
        return None;
    }
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
    if !is_ident(bytes[relative]) {
        return None;
    }

    let mut start = relative;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = relative;
    while end < bytes.len() && is_ident(bytes[end]) {
        end += 1;
    }
    let ident = text.get(start..end)?.to_owned();
    Some((
        SourceSpan::new(span.file_id, span.start + start, span.start + end),
        ident,
    ))
}

fn directive_spec(name: &str) -> Option<DirectiveSpec> {
    match name {
        "@import" => Some(DirectiveSpec {
            name: "@import",
            summary: "Import a local module and optionally bind it to an alias.",
            example: "@import \"./shared.luma\" as shared",
            excerpt: "Relative paths and `file:` URIs are allowed; non-`file:` schemes, absolute paths, and parent traversal are rejected by default in parse-only mode.",
        }),
        "@include" => Some(DirectiveSpec {
            name: "@include",
            summary: "Include a local document without evaluating Lua.",
            example: "@include \"./partials/base.luma\"",
            excerpt: "Includes stay root-bounded and parse-only unless a future evaluation mode is explicitly enabled.",
        }),
        "@use" => Some(DirectiveSpec {
            name: "@use",
            summary: "Bind a local module path to a reusable alias.",
            example: "@use \"./modules/network.luma\" as network",
            excerpt: "Module aliases are resolved from static import syntax; hover does not execute module code.",
        }),
        "@profile" => Some(DirectiveSpec {
            name: "@profile",
            summary: "Select the active profile name for the document.",
            example: "@profile dev",
            excerpt: "Profiles are treated as static identifiers in parse-only mode.",
        }),
        "@luma" => Some(DirectiveSpec {
            name: "@luma",
            summary: "Declare the Luma language version used by the file.",
            example: "@luma 1",
            excerpt: "Version declarations are parsed statically and never trigger evaluation.",
        }),
        _ => None,
    }
}

struct DirectiveSpec {
    name: &'static str,
    summary: &'static str,
    example: &'static str,
    excerpt: &'static str,
}

fn contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

fn node_type_name(node: &Node) -> &'static str {
    match node {
        Node::Mapping(_) => "mapping",
        Node::Sequence(_) => "sequence",
        Node::Scalar(scalar) => scalar_type_name(scalar),
        Node::Tag(_) => "tag",
        Node::Spread(_) => "spread",
        Node::Conditional(_) => "conditional",
        Node::Loop(_) => "loop",
        Node::Error(_) => "error",
    }
}

fn scalar_type_name(scalar: &Scalar) -> &'static str {
    match scalar.kind {
        ScalarKind::Plain => match scalar.text.as_str() {
            "true" | "false" => "boolean",
            "null" | "nil" => "null",
            _ => "plain",
        },
        ScalarKind::String | ScalarKind::BlockString => "string",
        ScalarKind::Number => "number",
        ScalarKind::LuaExpression => "lua-expression",
        ScalarKind::LuaBlock => "lua-block",
    }
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn escape_inline_code(value: &str) -> String {
    value.replace('`', "\\`")
}

fn trimmed_value_span(source: &crate::syntax::SourceText, span: SourceSpan) -> Option<SourceSpan> {
    let text = source.slice(span)?;
    let trimmed_start = text.len().saturating_sub(text.trim_start().len());
    let trimmed_end = text.trim_end().len();
    Some(SourceSpan::new(
        span.file_id,
        span.start + trimmed_start,
        span.start + trimmed_end,
    ))
}

fn key_path(source: &crate::syntax::SourceText, entry: &MappingEntry) -> Option<String> {
    let mut stack: Vec<(usize, String)> = vec![(0, "document[1]".to_owned())];
    for (start, line) in source.as_str().split('\n').scan(0usize, |offset, line| {
        let start = *offset;
        *offset += line.len() + 1;
        Some((start, line))
    }) {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('@')
            || trimmed.starts_with("let ")
        {
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let Some((raw_key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        if raw_key.starts_with('-') {
            continue;
        }
        while stack.len() > 1
            && stack
                .last()
                .is_some_and(|(stack_indent, _)| *stack_indent >= indent + 1)
        {
            stack.pop();
        }
        let key = raw_key.trim().to_owned();
        let key_start = start + line.find(raw_key.trim())?;
        if key_start == entry.key_span.start {
            let mut parts = stack
                .iter()
                .map(|(_, part)| part.clone())
                .collect::<Vec<_>>();
            parts.push(key);
            return Some(parts.join("."));
        }
        if raw_value.trim().is_empty() {
            stack.push((indent + 1, key));
        }
    }
    None
}
