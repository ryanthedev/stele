//! The syntax `Capture` role set: a small, fixed vocabulary of syntactic
//! roles that lumis's ~293 dot-hierarchical tree-sitter scope names
//! (`crates/highlight` never needs the full list — see
//! `docs/spikes/highlight-engine.md`) collapse onto. Each role is one
//! [`layout::StyleId::Capture`] value, so it gets exactly one themed color;
//! collapsing many scope names (`"keyword.import.rust"`,
//! `"keyword.conditional"`, …) onto few roles is what keeps the DW-7.2
//! distinct-color set small enough to guarantee no collisions.

/// A syntax-highlighting role. `Plain` is the catch-all for any scope name
/// this crate doesn't recognize (a future lumis scope, a language-specific
/// oddity) — it always resolves to the same styling as unhighlighted code,
/// so an unmapped scope degrades safely instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capture {
    Attribute,
    Boolean,
    Comment,
    CommentDoc,
    Constant,
    Constructor,
    Error,
    Function,
    FunctionMacro,
    Keyword,
    KeywordControl,
    Label,
    Namespace,
    Number,
    Operator,
    Property,
    Punctuation,
    String,
    StringEscape,
    Tag,
    Type,
    TypeBuiltin,
    Variable,
    VariableBuiltin,
    Plain,
}

/// Every [`Capture`] variant, in the exact order [`Capture::id`] assigns.
/// Kept as the single source of truth so id assignment, the color palette
/// size, and the distinctness test all derive from one list.
pub const ALL: [Capture; 25] = [
    Capture::Attribute,
    Capture::Boolean,
    Capture::Comment,
    Capture::CommentDoc,
    Capture::Constant,
    Capture::Constructor,
    Capture::Error,
    Capture::Function,
    Capture::FunctionMacro,
    Capture::Keyword,
    Capture::KeywordControl,
    Capture::Label,
    Capture::Namespace,
    Capture::Number,
    Capture::Operator,
    Capture::Property,
    Capture::Punctuation,
    Capture::String,
    Capture::StringEscape,
    Capture::Tag,
    Capture::Type,
    Capture::TypeBuiltin,
    Capture::Variable,
    Capture::VariableBuiltin,
    Capture::Plain,
];

impl Capture {
    /// The `crates/layout` `StyleId::Capture` id this role allocates. Stable
    /// for the life of this crate — a `Run` carrying this id must always
    /// resolve back to the same role.
    pub fn id(self) -> u16 {
        ALL.iter()
            .position(|&c| c == self)
            .expect("Capture::ALL is exhaustive over Capture") as u16
    }

    /// Resolves a `StyleId::Capture` id back to its role. An id past
    /// `ALL`'s range (which cannot happen from this crate's own output, but
    /// a `Decor::resolve` caller must still never panic on it — theme
    /// resolution is a total function over its input) degrades to `Plain`.
    pub fn from_id(id: u16) -> Capture {
        ALL.get(id as usize).copied().unwrap_or(Capture::Plain)
    }

    /// Maps one of lumis's raw tree-sitter scope names (e.g.
    /// `"keyword.import.rust"`, `"punctuation.bracket.rust"`) to a role.
    /// Matching is prefix-based over the scope's dot segments so a
    /// language-specific specialization (`".rust"`, `".python"`, …) or a
    /// finer sub-role (`".builtin"`, `".escape"`) still lands on its parent
    /// role. Order matters: more specific prefixes are listed first so
    /// they win over their own parents.
    pub fn from_scope(scope: &str) -> Capture {
        const TABLE: &[(&str, Capture)] = &[
            ("keyword.function", Capture::Keyword),
            ("keyword.type", Capture::Type),
            ("keyword.operator", Capture::Operator),
            ("keyword.conditional", Capture::KeywordControl),
            ("keyword.repeat", Capture::KeywordControl),
            ("keyword.return", Capture::KeywordControl),
            ("keyword.exception", Capture::KeywordControl),
            ("keyword.import", Capture::KeywordControl),
            ("keyword.export", Capture::KeywordControl),
            ("keyword.coroutine", Capture::KeywordControl),
            ("keyword.debug", Capture::KeywordControl),
            ("keyword.directive", Capture::KeywordControl),
            ("keyword.modifier", Capture::Keyword),
            ("keyword", Capture::Keyword),
            ("function.macro", Capture::FunctionMacro),
            ("function.builtin", Capture::Function),
            ("function.call", Capture::Function),
            ("function.method", Capture::Function),
            ("function", Capture::Function),
            ("type.builtin", Capture::TypeBuiltin),
            ("type.definition", Capture::Type),
            ("type", Capture::Type),
            ("string.escape", Capture::StringEscape),
            ("string.special", Capture::StringEscape),
            ("string.regex", Capture::StringEscape),
            ("string.regexp", Capture::StringEscape),
            ("string", Capture::String),
            ("character.special", Capture::StringEscape),
            ("character", Capture::String),
            ("number", Capture::Number),
            ("boolean", Capture::Boolean),
            ("constant.builtin", Capture::Constant),
            ("constant.macro", Capture::FunctionMacro),
            ("constant", Capture::Constant),
            ("comment.documentation", Capture::CommentDoc),
            ("comment", Capture::Comment),
            ("operator", Capture::Operator),
            ("punctuation.bracket", Capture::Punctuation),
            ("punctuation.delimiter", Capture::Punctuation),
            ("punctuation.special", Capture::Punctuation),
            ("punctuation", Capture::Punctuation),
            ("variable.builtin", Capture::VariableBuiltin),
            ("variable.parameter", Capture::Variable),
            ("variable.member", Capture::Property),
            ("variable", Capture::Variable),
            ("property", Capture::Property),
            ("namespace", Capture::Namespace),
            ("module", Capture::Namespace),
            ("attribute", Capture::Attribute),
            ("tag.attribute", Capture::Attribute),
            ("tag", Capture::Tag),
            ("constructor", Capture::Constructor),
            ("label", Capture::Label),
            ("error", Capture::Error),
        ];
        TABLE
            .iter()
            .find(|(prefix, _)| scope_matches(scope, prefix))
            .map_or(Capture::Plain, |&(_, role)| role)
    }
}

/// `scope` matches `prefix` exactly, or `prefix` followed by a `.`-prefixed
/// specialization (`"keyword.import.rust"` matches prefix `"keyword.import"`).
/// A bare textual `starts_with` would also match an unrelated scope that
/// merely shares a substring (`"keyworder"`); requiring a following `.` or
/// end-of-string rules that out.
fn scope_matches(scope: &str, prefix: &str) -> bool {
    scope
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ids_are_dense_and_roundtrip() {
        for (i, &role) in ALL.iter().enumerate() {
            assert_eq!(role.id(), i as u16);
            assert_eq!(Capture::from_id(i as u16), role);
        }
    }

    #[test]
    fn test_from_id_out_of_range_degrades_to_plain() {
        assert_eq!(Capture::from_id(u16::MAX), Capture::Plain);
    }

    #[test]
    fn test_from_scope_maps_common_language_specific_scopes() {
        assert_eq!(
            Capture::from_scope("keyword.import.rust"),
            Capture::KeywordControl
        );
        assert_eq!(
            Capture::from_scope("punctuation.bracket.rust"),
            Capture::Punctuation
        );
        assert_eq!(
            Capture::from_scope("type.builtin.python"),
            Capture::TypeBuiltin
        );
        assert_eq!(
            Capture::from_scope("variable.builtin.javascript"),
            Capture::VariableBuiltin
        );
        assert_eq!(
            Capture::from_scope("string.special.url.html"),
            Capture::StringEscape
        );
        assert_eq!(
            Capture::from_scope("comment.documentation"),
            Capture::CommentDoc
        );
        assert_eq!(
            Capture::from_scope("constant.builtin.rust"),
            Capture::Constant
        );
    }

    #[test]
    fn test_from_scope_exact_bare_names() {
        assert_eq!(Capture::from_scope("keyword"), Capture::Keyword);
        assert_eq!(Capture::from_scope("string"), Capture::String);
        assert_eq!(Capture::from_scope("number"), Capture::Number);
        assert_eq!(Capture::from_scope("operator"), Capture::Operator);
        assert_eq!(Capture::from_scope("function"), Capture::Function);
    }

    #[test]
    fn test_from_scope_unknown_scope_is_plain_not_a_panic() {
        assert_eq!(Capture::from_scope(""), Capture::Plain);
        assert_eq!(
            Capture::from_scope("markup.heading.1.markdown"),
            Capture::Plain
        );
        assert_eq!(Capture::from_scope("keywordxyz"), Capture::Plain);
    }
}
