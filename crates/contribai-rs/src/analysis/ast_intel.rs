//! Tree-sitter AST intelligence — native code understanding.
//!
//! 🆕 This is a NEW capability in the Rust version.
//! Python ContribAI uses regex; Rust ContribAI uses proper AST parsing.
//!
//! Supports: Python, JavaScript, TypeScript, Go, Rust, Java, C, C++.

use tracing::debug;

use crate::core::error::{ContribError, Result};
use crate::core::models::{Symbol, SymbolKind};

/// Supported languages for AST parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Go,
    Rust,
    Java,
    C,
    Cpp,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "py" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "tsx" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "rs" => Some(Self::Rust),
            "java" | "kt" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(Self::Cpp),
            _ => None,
        }
    }

    /// Get language from repo's primary language string.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "python" => Some(Self::Python),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "rust" => Some(Self::Rust),
            "java" | "kotlin" => Some(Self::Java),
            "c" => Some(Self::C),
            "c++" | "cpp" => Some(Self::Cpp),
            _ => None,
        }
    }
}

/// AST intelligence engine powered by tree-sitter.
pub struct AstIntel;

impl AstIntel {
    /// Extract symbols (functions, classes, methods, etc.) from source code.
    pub fn extract_symbols(source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let ext = file_path
            .rsplit('.')
            .next()
            .unwrap_or("");

        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => {
                debug!(path = file_path, "No AST parser for extension");
                return Ok(vec![]);
            }
        };

        let mut parser = tree_sitter::Parser::new();
        let ts_lang = Self::get_ts_language(lang)?;
        parser
            .set_language(&ts_lang)
            .map_err(|e| ContribError::AstParse(format!("Failed to set language: {}", e)))?;

        let tree = parser
            .parse(source, None)
            .ok_or_else(|| ContribError::AstParse("Parse failed".into()))?;

        let root = tree.root_node();
        let mut symbols = Vec::new();
        Self::walk_node(root, source, file_path, lang, &mut symbols);

        debug!(
            path = file_path,
            count = symbols.len(),
            "Extracted symbols"
        );
        Ok(symbols)
    }

    /// Get the tree-sitter Language for a given language.
    fn get_ts_language(lang: Language) -> Result<tree_sitter::Language> {
        let ts_lang = match lang {
            Language::Python => tree_sitter_python::LANGUAGE,
            Language::JavaScript => tree_sitter_javascript::LANGUAGE,
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            Language::Go => tree_sitter_go::LANGUAGE,
            Language::Rust => tree_sitter_rust::LANGUAGE,
            Language::Java => tree_sitter_java::LANGUAGE,
            Language::C => tree_sitter_c::LANGUAGE,
            Language::Cpp => tree_sitter_cpp::LANGUAGE,
        };
        Ok(ts_lang.into())
    }

    /// Recursively walk AST nodes and extract symbols.
    fn walk_node(
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        lang: Language,
        symbols: &mut Vec<Symbol>,
    ) {
        let kind = node.kind();

        // Map node kinds to symbol kinds based on language
        let symbol_kind = match lang {
            Language::Python => match kind {
                "function_definition" => Some(SymbolKind::Function),
                "class_definition" => Some(SymbolKind::Class),
                "import_statement" | "import_from_statement" => Some(SymbolKind::Import),
                _ => None,
            },
            Language::JavaScript | Language::TypeScript => match kind {
                "function_declaration" | "arrow_function" | "generator_function_declaration" => {
                    Some(SymbolKind::Function)
                }
                "class_declaration" => Some(SymbolKind::Class),
                "method_definition" => Some(SymbolKind::Method),
                "interface_declaration" => Some(SymbolKind::Interface),
                "enum_declaration" => Some(SymbolKind::Enum),
                "import_statement" => Some(SymbolKind::Import),
                "lexical_declaration" => {
                    // Check if it's a const with uppercase name (likely a constant)
                    if let Some(name) = Self::extract_name(node, source) {
                        if name.chars().all(|c| c.is_uppercase() || c == '_') {
                            Some(SymbolKind::Constant)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            },
            Language::Go => match kind {
                "function_declaration" => Some(SymbolKind::Function),
                "method_declaration" => Some(SymbolKind::Method),
                "type_declaration" => Some(SymbolKind::Struct),
                "import_declaration" => Some(SymbolKind::Import),
                _ => None,
            },
            Language::Rust => match kind {
                "function_item" => Some(SymbolKind::Function),
                "struct_item" => Some(SymbolKind::Struct),
                "enum_item" => Some(SymbolKind::Enum),
                "impl_item" => Some(SymbolKind::Class),
                "trait_item" => Some(SymbolKind::Interface),
                "use_declaration" => Some(SymbolKind::Import),
                "const_item" | "static_item" => Some(SymbolKind::Constant),
                _ => None,
            },
            Language::Java => match kind {
                "method_declaration" => Some(SymbolKind::Method),
                "class_declaration" => Some(SymbolKind::Class),
                "interface_declaration" => Some(SymbolKind::Interface),
                "enum_declaration" => Some(SymbolKind::Enum),
                "import_declaration" => Some(SymbolKind::Import),
                _ => None,
            },
            Language::C | Language::Cpp => match kind {
                "function_definition" | "function_declarator" => Some(SymbolKind::Function),
                "struct_specifier" => Some(SymbolKind::Struct),
                "enum_specifier" => Some(SymbolKind::Enum),
                "class_specifier" => Some(SymbolKind::Class),
                "preproc_include" => Some(SymbolKind::Import),
                _ => None,
            },
        };

        if let Some(sk) = symbol_kind {
            if let Some(name) = Self::extract_name(node, source) {
                symbols.push(Symbol {
                    name,
                    kind: sk,
                    file_path: file_path.to_string(),
                    line_start: node.start_position().row + 1,
                    line_end: node.end_position().row + 1,
                });
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_node(child, source, file_path, lang, symbols);
        }
    }

    /// Extract the name of a symbol from its AST node.
    fn extract_name(node: tree_sitter::Node, source: &str) -> Option<String> {
        // Try common name child node types
        let name_kinds = ["name", "identifier", "property_identifier", "type_identifier"];
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if name_kinds.contains(&child.kind()) {
                let text = &source[child.byte_range()];
                return Some(text.to_string());
            }
        }

        // Fallback: use the node text itself (truncated)
        let text = &source[node.byte_range()];
        let first_line = text.lines().next().unwrap_or(text);
        if first_line.len() <= 80 {
            Some(first_line.to_string())
        } else {
            None
        }
    }

    /// Count imports in a file (for PageRank edge weights).
    pub fn count_imports(source: &str, file_path: &str) -> Vec<String> {
        let symbols = Self::extract_symbols(source, file_path).unwrap_or_default();
        symbols
            .into_iter()
            .filter(|s| s.kind == SymbolKind::Import)
            .map(|s| s.name)
            .collect()
    }

    /// Get a summary of symbols as a compact string for LLM context.
    pub fn symbols_summary(symbols: &[Symbol]) -> String {
        if symbols.is_empty() {
            return "No symbols extracted.".to_string();
        }

        let mut lines = Vec::new();
        for s in symbols.iter().take(50) {
            lines.push(format!(
                "  {:?} {} (L{}-{})",
                s.kind, s.name, s.line_start, s.line_end
            ));
        }
        if symbols.len() > 50 {
            lines.push(format!("  ... +{} more", symbols.len() - 50));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_extension() {
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("js"), Some(Language::JavaScript));
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("go"), Some(Language::Go));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("java"), Some(Language::Java));
        assert_eq!(Language::from_extension("c"), Some(Language::C));
        assert_eq!(Language::from_extension("cpp"), Some(Language::Cpp));
        assert_eq!(Language::from_extension("md"), None);
    }

    #[test]
    fn test_extract_python_symbols() {
        let source = r#"
import os
from pathlib import Path

class MyClass:
    def __init__(self):
        pass

    def method(self):
        pass

def standalone_func():
    return 42

CONSTANT = "hello"
"#;
        let symbols = AstIntel::extract_symbols(source, "test.py").unwrap();

        let funcs: Vec<_> = symbols.iter().filter(|s| s.kind == SymbolKind::Function).collect();
        let classes: Vec<_> = symbols.iter().filter(|s| s.kind == SymbolKind::Class).collect();
        let imports: Vec<_> = symbols.iter().filter(|s| s.kind == SymbolKind::Import).collect();

        assert!(funcs.len() >= 1, "Should find standalone_func");
        assert_eq!(classes.len(), 1, "Should find MyClass");
        assert!(imports.len() >= 1, "Should find imports");
    }

    #[test]
    fn test_extract_rust_symbols() {
        let source = r#"
use std::collections::HashMap;

const MAX_SIZE: usize = 100;

struct Config {
    name: String,
}

enum Status {
    Active,
    Inactive,
}

fn process(config: &Config) -> Status {
    Status::Active
}

impl Config {
    fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}
"#;
        let symbols = AstIntel::extract_symbols(source, "test.rs").unwrap();

        let has_struct = symbols.iter().any(|s| s.kind == SymbolKind::Struct && s.name == "Config");
        let has_enum = symbols.iter().any(|s| s.kind == SymbolKind::Enum && s.name == "Status");
        let has_func = symbols.iter().any(|s| s.kind == SymbolKind::Function && s.name == "process");
        let has_import = symbols.iter().any(|s| s.kind == SymbolKind::Import);

        assert!(has_struct, "Should find Config struct");
        assert!(has_enum, "Should find Status enum");
        assert!(has_func, "Should find process function");
        assert!(has_import, "Should find use declaration");
    }

    #[test]
    fn test_extract_javascript_symbols() {
        let source = r#"
import { useState } from 'react';

class Component {
    render() { return null; }
}

function handleClick(event) {
    console.log(event);
}
"#;
        let symbols = AstIntel::extract_symbols(source, "test.js").unwrap();
        assert!(!symbols.is_empty(), "Should extract JS symbols");
    }

    #[test]
    fn test_unknown_extension() {
        let symbols = AstIntel::extract_symbols("hello", "test.unknown").unwrap();
        assert!(symbols.is_empty(), "Unknown extension should return empty");
    }

    #[test]
    fn test_symbols_summary() {
        let symbols = vec![
            Symbol {
                name: "main".into(),
                kind: SymbolKind::Function,
                file_path: "main.rs".into(),
                line_start: 1,
                line_end: 10,
            },
            Symbol {
                name: "Config".into(),
                kind: SymbolKind::Struct,
                file_path: "config.rs".into(),
                line_start: 5,
                line_end: 20,
            },
        ];
        let summary = AstIntel::symbols_summary(&symbols);
        assert!(summary.contains("main"));
        assert!(summary.contains("Config"));
    }
}
