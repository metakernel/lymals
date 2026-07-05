pub mod ast;
pub mod capabilities;
pub mod code_actions;
pub mod commands;
pub mod completion;
pub mod config;
pub mod diagnostics;
pub mod document;
pub mod eval;
pub mod folding;
pub mod formatting;
pub mod hover;
pub mod imports;
pub mod index;
pub mod inlay_hints;
pub mod lexer;
pub mod navigation;
pub mod parser;
pub mod position;
pub mod references;
pub mod rename;
pub mod selection_ranges;
pub mod semantic;
pub mod semantic_tokens;
pub mod server;
pub mod state;
pub mod symbols;
pub mod syntax;
pub mod tasks;
pub mod workspace;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn version_banner() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), VERSION)
}
