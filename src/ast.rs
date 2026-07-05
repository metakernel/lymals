use crate::syntax::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstFile {
    pub span: SourceSpan,
    pub documents: Vec<Document>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub span: SourceSpan,
    pub separator_span: Option<SourceSpan>,
    pub items: Vec<DocumentItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentItem {
    Directive(Directive),
    Comment(Comment),
    Let(LetBinding),
    Node(Node),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub name: String,
    pub span: SourceSpan,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub span: SourceSpan,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetBinding {
    pub name: String,
    pub span: SourceSpan,
    pub value_span: Option<SourceSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Mapping(Mapping),
    Sequence(Sequence),
    Scalar(Scalar),
    Tag(TagNode),
    Spread(SpreadNode),
    Conditional(ConditionalNode),
    Loop(LoopNode),
    Error(ErrorNode),
}

impl Node {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Mapping(node) => node.span,
            Self::Sequence(node) => node.span,
            Self::Scalar(node) => node.span,
            Self::Tag(node) => node.span,
            Self::Spread(node) => node.span,
            Self::Conditional(node) => node.span,
            Self::Loop(node) => node.span,
            Self::Error(node) => node.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub span: SourceSpan,
    pub entries: Vec<MappingEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingEntry {
    pub key: String,
    pub key_span: SourceSpan,
    pub span: SourceSpan,
    pub value: Option<Box<Node>>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub span: SourceSpan,
    pub items: Vec<SequenceItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceItem {
    pub span: SourceSpan,
    pub value: Option<Box<Node>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scalar {
    pub kind: ScalarKind,
    pub span: SourceSpan,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarKind {
    Plain,
    String,
    Number,
    BlockString,
    LuaExpression,
    LuaBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagNode {
    pub name: String,
    pub span: SourceSpan,
    pub value: Option<Box<Node>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpreadNode {
    pub span: SourceSpan,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalNode {
    pub span: SourceSpan,
    pub condition: String,
    pub branches: Vec<ConditionalBranch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalBranch {
    pub label: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopNode {
    pub span: SourceSpan,
    pub header: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorNode {
    pub span: SourceSpan,
    pub message: String,
}
