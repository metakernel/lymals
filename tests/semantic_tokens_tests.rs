use lumals::{
    capabilities,
    semantic_tokens::{self, TOKEN_MODIFIERS, TOKEN_TYPES},
    syntax::FileId,
};
use tower_lsp::lsp_types::{
    ClientCapabilities, SemanticTokensFullOptions, SemanticTokensServerCapabilities,
};

#[test]
fn semantic_tokens_provider_is_advertised_with_full_support() {
    let capabilities = capabilities::negotiate(&ClientCapabilities::default());
    let provider = capabilities
        .semantic_tokens_provider
        .expect("semantic tokens provider should be advertised");

    match provider {
        SemanticTokensServerCapabilities::SemanticTokensOptions(options) => {
            assert!(matches!(
                options.full,
                Some(SemanticTokensFullOptions::Bool(true))
            ));
            assert!(options.range.is_none());
        }
        other => panic!("unexpected semantic tokens capability: {other:?}"),
    }
}

#[test]
fn semantic_tokens_legend_snapshot_is_stable() {
    let legend = semantic_tokens::legend();
    let actual = format!(
        "tokenTypes: {}\ntokenModifiers: {}\nlegend.tokenTypes: {}\nlegend.tokenModifiers: {}\n",
        TOKEN_TYPES.join(", "),
        TOKEN_MODIFIERS.join(", "),
        legend
            .token_types
            .iter()
            .map(|token| token.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        legend
            .token_modifiers
            .iter()
            .map(|modifier| modifier.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );

    insta::assert_snapshot!(actual, @r###"
    tokenTypes: keyword, property, string, number, comment, operator, decorator, variable, macro, boolean, null, lua
    tokenModifiers: declaration, block, import, include, deprecated, invalid
    legend.tokenTypes: keyword, property, string, number, comment, operator, decorator, variable, macro, boolean, null, lua
    legend.tokenModifiers: declaration, block, import, include, deprecated, invalid
    "###);
}

#[test]
fn semantic_tokens_snapshot_captures_delta_encoded_output() {
    let text = concat!(
        "---\n",
        "@luma 1\n",
        "@import \"./shared.luma\" as shared\n",
        "@include partials/base.luma\n",
        "@use \"./legacy.luma\" as legacy\n",
        "let enabled = true\n",
        "let nothing = nil\n",
        "let count = 3.14\n",
        "message: \"hello\"\n",
        "block: |\n",
        "  first line\n",
        "  ${lua + 1}\n",
        "calc: = service.port + 1\n",
        "script: lua{return shared.region}\n",
        "tagged:\n",
        "- !lambda worker\n",
        "ref: ${shared.value}\n",
        "invalid: \"unterminated\n",
        "legacy: .\n",
        "# trailing comment\n",
    );

    let tokens = semantic_tokens::tokenize(FileId(33), "fixture.luma", text);
    let rendered = semantic_tokens::debug_render(text, &tokens);
    let actual = tokens
        .data
        .iter()
        .zip(rendered.iter())
        .map(|(token, rendered)| {
            format!(
                "[{},{},{},{},{}] {} {:?} {}",
                token.delta_line,
                token.delta_start,
                token.length,
                token.token_type,
                token.token_modifiers_bitset,
                rendered.token_type,
                rendered.modifiers,
                rendered.lexeme,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    insta::assert_snapshot!(actual, @r###"
    [0,0,3,5,0] operator [] ---
    [1,0,5,6,0] decorator [] @luma
    [0,6,1,3,0] number [] 1
    [1,0,7,6,0] decorator [] @import
    [0,8,15,2,4] string ["import"] "./shared.luma"
    [0,16,2,0,0] keyword [] as
    [0,3,6,7,1] variable ["declaration"] shared
    [1,0,8,6,0] decorator [] @include
    [0,9,18,2,8] string ["include"] partials/base.luma
    [1,0,4,6,16] decorator ["deprecated"] @use
    [0,5,15,2,20] string ["import", "deprecated"] "./legacy.luma"
    [0,16,2,0,0] keyword [] as
    [0,3,6,7,17] variable ["declaration", "deprecated"] legacy
    [1,0,3,0,0] keyword [] let
    [0,4,7,7,1] variable ["declaration"] enabled
    [0,8,1,5,0] operator [] =
    [0,2,4,9,0] boolean [] true
    [1,0,3,0,0] keyword [] let
    [0,4,7,7,1] variable ["declaration"] nothing
    [0,8,1,5,0] operator [] =
    [0,2,3,10,0] null [] nil
    [1,0,3,0,0] keyword [] let
    [0,4,5,7,1] variable ["declaration"] count
    [0,6,1,5,0] operator [] =
    [0,2,4,3,0] number [] 3.14
    [1,0,7,1,0] property [] message
    [0,7,1,5,0] operator [] :
    [0,2,7,2,0] string [] "hello"
    [1,0,5,1,0] property [] block
    [0,5,1,5,0] operator [] :
    [0,2,1,5,0] operator [] |
    [1,2,10,2,2] string ["block"] first line
    [1,2,10,11,2] lua ["block"] ${lua + 1}
    [1,0,4,1,0] property [] calc
    [0,4,1,5,0] operator [] :
    [0,2,18,11,0] lua [] = service.port + 1
    [1,0,6,1,0] property [] script
    [0,6,1,5,0] operator [] :
    [0,2,25,11,0] lua [] lua{return shared.region}
    [1,0,6,1,0] property [] tagged
    [0,6,1,5,0] operator [] :
    [1,0,1,5,0] operator [] -
    [0,2,7,8,0] macro [] !lambda
    [0,8,6,2,0] string [] worker
    [1,0,3,1,0] property [] ref
    [0,3,1,5,0] operator [] :
    [0,2,15,11,0] lua [] ${shared.value}
    [1,0,7,1,0] property [] invalid
    [0,7,1,5,0] operator [] :
    [0,2,13,2,32] string ["invalid"] "unterminated
    [1,0,6,1,0] property [] legacy
    [0,6,1,5,0] operator [] :
    [0,2,1,5,32] operator ["invalid"] .
    [1,0,18,4,0] comment [] # trailing comment
    "###);
}
