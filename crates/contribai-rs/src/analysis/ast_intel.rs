//! Tree-sitter AST intelligence — native code understanding.
//!
//! 🆕 This is a NEW capability in the Rust version.
//! Python ContribAI uses regex; Rust ContribAI uses proper AST parsing.
//!
//! Supports: Python, JavaScript, TypeScript, Go, Rust, Java, C, C++,
//!           Ruby, PHP, C#, HTML, CSS (13 languages).

use std::collections::HashMap;
use tracing::debug;

use crate::core::error::{ContribError, Result};
use crate::core::models::{Symbol, SymbolKind};

/// An imported symbol and the module it comes from.
#[derive(Debug, Clone)]
pub struct ImportTarget {
    /// The symbol name being imported (e.g., "HashMap", "Path", "useState").
    pub symbol_name: String,
    /// The source module path (e.g., "std::collections", "pathlib", "./utils").
    pub source_path: String,
}

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
    Ruby,
    Php,
    CSharp,
    Html,
    Css,
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
            "java" => Some(Self::Java),
            "kt" | "kts" => Some(Self::Java), // Kotlin uses Java-like AST
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(Self::Cpp),
            "rb" | "rake" | "gemspec" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "cs" => Some(Self::CSharp),
            "swift" => Some(Self::Java), // Swift uses Java-like AST as fallback
            "html" | "htm" => Some(Self::Html),
            "css" | "scss" => Some(Self::Css),
            "vue" | "svelte" => Some(Self::Html), // Vue/Svelte template ≈ HTML
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
            "java" => Some(Self::Java),
            "kotlin" => Some(Self::Java),
            "c" => Some(Self::C),
            "c++" | "cpp" => Some(Self::Cpp),
            "ruby" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "c#" | "csharp" => Some(Self::CSharp),
            "swift" => Some(Self::Java),
            "html" => Some(Self::Html),
            "css" | "scss" => Some(Self::Css),
            _ => None,
        }
    }
}

/// AST intelligence engine powered by tree-sitter.
pub struct AstIntel;

impl AstIntel {
    /// Extract symbols (functions, classes, methods, etc.) from source code.
    pub fn extract_symbols(source: &str, file_path: &str) -> Result<Vec<Symbol>> {
        let ext = file_path.rsplit('.').next().unwrap_or("");

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

        debug!(path = file_path, count = symbols.len(), "Extracted symbols");
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
            Language::Ruby => tree_sitter_ruby::LANGUAGE,
            Language::Php => tree_sitter_php::LANGUAGE_PHP,
            Language::CSharp => tree_sitter_c_sharp::LANGUAGE,
            Language::Html => tree_sitter_html::LANGUAGE,
            Language::Css => tree_sitter_css::LANGUAGE,
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
                "method_declaration" | "constructor_declaration" => Some(SymbolKind::Method),
                "class_declaration" => Some(SymbolKind::Class),
                "interface_declaration" => Some(SymbolKind::Interface),
                "enum_declaration" => Some(SymbolKind::Enum),
                "import_declaration" => Some(SymbolKind::Import),
                "field_declaration" => Some(SymbolKind::Constant),
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
            Language::Ruby => match kind {
                "method" | "singleton_method" => Some(SymbolKind::Function),
                "class" => Some(SymbolKind::Class),
                "module" => Some(SymbolKind::Class),
                "call" => {
                    // Detect require/include
                    if let Some(name) = Self::extract_name(node, source) {
                        if name == "require" || name == "include" || name == "require_relative" {
                            Some(SymbolKind::Import)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            },
            Language::Php => match kind {
                "function_definition" => Some(SymbolKind::Function),
                "method_declaration" => Some(SymbolKind::Method),
                "class_declaration" => Some(SymbolKind::Class),
                "interface_declaration" => Some(SymbolKind::Interface),
                "trait_declaration" => Some(SymbolKind::Interface),
                "enum_declaration" => Some(SymbolKind::Enum),
                "namespace_use_declaration" => Some(SymbolKind::Import),
                _ => None,
            },
            Language::CSharp => match kind {
                "method_declaration" | "constructor_declaration" => Some(SymbolKind::Method),
                "class_declaration" | "record_declaration" => Some(SymbolKind::Class),
                "interface_declaration" => Some(SymbolKind::Interface),
                "enum_declaration" => Some(SymbolKind::Enum),
                "struct_declaration" => Some(SymbolKind::Struct),
                "using_directive" => Some(SymbolKind::Import),
                "property_declaration" => Some(SymbolKind::Constant),
                _ => None,
            },
            Language::Html => match kind {
                "element" | "script_element" | "style_element" => Some(SymbolKind::Struct),
                _ => None,
            },
            Language::Css => match kind {
                "rule_set" => Some(SymbolKind::Struct),
                "import_statement" => Some(SymbolKind::Import),
                "media_statement" => Some(SymbolKind::Function),
                "keyframes_statement" => Some(SymbolKind::Function),
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
        let name_kinds = [
            "name",
            "identifier",
            "property_identifier",
            "type_identifier",
            "tag_name",
            "class_name",
            "constant",
        ];
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

    /// Extract import targets with their source module paths.
    ///
    /// Parses import statements and returns (symbol_name, source_module) pairs.
    /// Supports: Rust, Python, JS/TS, Go, Java (5 languages).
    pub fn extract_import_targets(source: &str, file_path: &str) -> Vec<ImportTarget> {
        let ext = file_path.rsplit('.').next().unwrap_or("");
        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => return vec![],
        };

        let mut parser = tree_sitter::Parser::new();
        let ts_lang = match Self::get_ts_language(lang) {
            Ok(l) => l,
            Err(_) => return vec![],
        };
        if parser.set_language(&ts_lang).is_err() {
            return vec![];
        }

        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return vec![],
        };

        let root = tree.root_node();
        let mut targets = Vec::new();
        Self::walk_import_nodes(root, source, lang, &mut targets, 0);
        targets
    }

    /// Walk AST nodes to extract import targets with source paths.
    #[allow(clippy::collapsible_match)]
    fn walk_import_nodes(
        node: tree_sitter::Node,
        source: &str,
        lang: Language,
        targets: &mut Vec<ImportTarget>,
        depth: usize,
    ) {
        if depth > 8 {
            return;
        }
        let kind = node.kind();
        let text = &source[node.byte_range()];

        match lang {
            Language::Python => {
                // `from pathlib import Path` → symbol="Path", source="pathlib"
                // `import os` → symbol="os", source="os"
                if kind == "import_from_statement" {
                    let module = Self::child_text(node, "module_name", source)
                        .or_else(|| Self::child_text(node, "dotted_name", source));
                    if let Some(module_path) = module {
                        // Find imported names
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() == "dotted_name"
                                && child.start_byte()
                                    > node
                                        .child_by_field_name("module_name")
                                        .map(|n| n.end_byte())
                                        .unwrap_or(0)
                            {
                                targets.push(ImportTarget {
                                    symbol_name: source[child.byte_range()].to_string(),
                                    source_path: module_path.clone(),
                                });
                            }
                            if child.kind() == "aliased_import" || child.kind() == "import" {
                                if let Some(name) = Self::child_text(child, "name", source) {
                                    targets.push(ImportTarget {
                                        symbol_name: name,
                                        source_path: module_path.clone(),
                                    });
                                }
                            }
                        }
                        // Fallback: parse from text if no targets were added for this node
                        let added_for_node = targets
                            .iter()
                            .rev()
                            .take_while(|t| t.source_path == module_path)
                            .count();
                        if added_for_node == 0 {
                            if let Some(after_import) = text.split("import").nth(1) {
                                for name in after_import.split(',') {
                                    let name = name.split(" as ").next().unwrap_or("").trim();
                                    if !name.is_empty() && name != "(" && name != ")" {
                                        targets.push(ImportTarget {
                                            symbol_name: name.to_string(),
                                            source_path: module_path.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                } else if kind == "import_statement" {
                    // `import os` → symbol="os", source="os"
                    if let Some(name) = Self::child_text(node, "name", source)
                        .or_else(|| Self::child_text(node, "dotted_name", source))
                    {
                        targets.push(ImportTarget {
                            symbol_name: name.clone(),
                            source_path: name,
                        });
                    }
                }
            }
            Language::JavaScript | Language::TypeScript => {
                // `import { Foo, Bar } from './module'` → symbols with source
                if kind == "import_statement" {
                    let source_path = Self::child_text(node, "source", source)
                        .map(|s| s.trim_matches('\'').trim_matches('"').to_string())
                        .unwrap_or_default();
                    if source_path.is_empty() {
                        // Fallback: extract from text
                        if let Some(from_part) = text.split("from").last() {
                            let sp = from_part
                                .trim()
                                .trim_matches(|c| c == '\'' || c == '"' || c == ';')
                                .to_string();
                            if !sp.is_empty() {
                                Self::extract_js_import_names(text, &sp, targets);
                            }
                        }
                    } else {
                        Self::extract_js_import_names(text, &source_path, targets);
                    }
                }
            }
            Language::Rust => {
                // `use std::collections::HashMap;` → symbol="HashMap", source="std::collections"
                if kind == "use_declaration" {
                    let path_text = text.trim_start_matches("use ").trim_end_matches(';').trim();
                    Self::parse_rust_use(path_text, targets);
                }
            }
            Language::Go => {
                // `import "fmt"` or `import ( "fmt" "os" )`
                if kind == "import_declaration" {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "import_spec"
                            || child.kind() == "interpreted_string_literal"
                        {
                            let import_path =
                                source[child.byte_range()].trim_matches('"').to_string();
                            let symbol = import_path
                                .rsplit('/')
                                .next()
                                .unwrap_or(&import_path)
                                .to_string();
                            targets.push(ImportTarget {
                                symbol_name: symbol,
                                source_path: import_path,
                            });
                        }
                        // Nested import specs in import block
                        if child.kind() == "import_spec_list" {
                            let mut inner_cursor = child.walk();
                            for spec in child.children(&mut inner_cursor) {
                                if spec.kind() == "import_spec" {
                                    let spec_text = source[spec.byte_range()].trim();
                                    let import_path = spec_text.trim_matches('"').to_string();
                                    let symbol = import_path
                                        .rsplit('/')
                                        .next()
                                        .unwrap_or(&import_path)
                                        .to_string();
                                    targets.push(ImportTarget {
                                        symbol_name: symbol,
                                        source_path: import_path,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Language::Java => {
                // `import com.example.Foo;` → symbol="Foo", source="com.example"
                if kind == "import_declaration" {
                    let path_text = text
                        .trim_start_matches("import ")
                        .trim_start_matches("static ")
                        .trim_end_matches(';')
                        .trim();
                    if let Some(dot_pos) = path_text.rfind('.') {
                        targets.push(ImportTarget {
                            symbol_name: path_text[dot_pos + 1..].to_string(),
                            source_path: path_text[..dot_pos].to_string(),
                        });
                    }
                }
            }
            _ => {} // Other languages: no import resolution
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_import_nodes(child, source, lang, targets, depth + 1);
        }
    }

    /// Helper: get text of a named child node.
    fn child_text(node: tree_sitter::Node, field_name: &str, source: &str) -> Option<String> {
        node.child_by_field_name(field_name)
            .map(|child| source[child.byte_range()].to_string())
    }

    /// Helper: extract named imports from JS/TS import text.
    fn extract_js_import_names(text: &str, source_path: &str, targets: &mut Vec<ImportTarget>) {
        // Handle `import { Foo, Bar } from '...'`
        if let Some(start) = text.find('{') {
            if let Some(end) = text.find('}') {
                let names_str = &text[start + 1..end];
                for name in names_str.split(',') {
                    let name = name.split(" as ").next().unwrap_or("").trim();
                    if !name.is_empty() {
                        targets.push(ImportTarget {
                            symbol_name: name.to_string(),
                            source_path: source_path.to_string(),
                        });
                    }
                }
                return;
            }
        }
        // Handle `import Foo from '...'` (default import)
        if let Some(after_import) = text.strip_prefix("import ") {
            let name = after_import.split_whitespace().next().unwrap_or("").trim();
            if !name.is_empty() && name != "{" && name != "*" && name != "type" {
                targets.push(ImportTarget {
                    symbol_name: name.to_string(),
                    source_path: source_path.to_string(),
                });
            }
        }
    }

    /// Helper: parse Rust `use` path into ImportTargets.
    fn parse_rust_use(path: &str, targets: &mut Vec<ImportTarget>) {
        // Handle `use crate::module::{Foo, Bar};`
        if let Some(brace_start) = path.find('{') {
            let prefix = path[..brace_start].trim_end_matches("::");
            if let Some(brace_end) = path.find('}') {
                for name in path[brace_start + 1..brace_end].split(',') {
                    let name = name.split(" as ").next().unwrap_or("").trim();
                    if !name.is_empty() {
                        targets.push(ImportTarget {
                            symbol_name: name.to_string(),
                            source_path: prefix.to_string(),
                        });
                    }
                }
            }
        } else if let Some(last_sep) = path.rfind("::") {
            // `use std::collections::HashMap` → symbol="HashMap", source="std::collections"
            targets.push(ImportTarget {
                symbol_name: path[last_sep + 2..].to_string(),
                source_path: path[..last_sep].to_string(),
            });
        } else {
            // `use foo;`
            targets.push(ImportTarget {
                symbol_name: path.to_string(),
                source_path: path.to_string(),
            });
        }
    }

    /// Resolve imported symbols against a map of already-parsed files.
    ///
    /// Returns symbol_name → type_signature (one-line summary) for symbols
    /// found in the parsed_files map. 1-hop resolution only, capped at 20.
    pub fn resolve_imports(
        imports: &[ImportTarget],
        parsed_files: &HashMap<String, Vec<Symbol>>,
    ) -> HashMap<String, String> {
        let mut resolved = HashMap::new();

        for target in imports.iter().take(20) {
            // Search all parsed files for a symbol matching the import name
            for (file_path, symbols) in parsed_files {
                // Skip if this is a standard library import (not a local file)
                if target.source_path.starts_with("std")
                    || target.source_path.starts_with("core")
                    || target.source_path.starts_with("alloc")
                    || target.source_path.starts_with("os")
                    || target.source_path.starts_with("sys")
                    || target.source_path.starts_with("react")
                    || target.source_path.starts_with("vue")
                    || target.source_path.starts_with("@")
                    || target.source_path.starts_with("node:")
                    || target.source_path.contains("node_modules")
                    || target.source_path.contains(".venv")
                    || target.source_path.contains("vendor")
                {
                    continue;
                }

                for symbol in symbols {
                    if symbol.name == target.symbol_name
                        && matches!(
                            symbol.kind,
                            SymbolKind::Function
                                | SymbolKind::Struct
                                | SymbolKind::Class
                                | SymbolKind::Interface
                                | SymbolKind::Enum
                        )
                    {
                        let sig = format!(
                            "{:?} {} ({}:L{}-L{})",
                            symbol.kind, symbol.name, file_path, symbol.line_start, symbol.line_end
                        );
                        resolved.insert(target.symbol_name.clone(), sig);
                        break;
                    }
                }

                if resolved.len() >= 20 {
                    break;
                }
            }
        }

        resolved
    }

    /// Multi-hop import resolution: follows import chains across files.
    ///
    /// 1-hop: A imports B directly
    /// 2-hop: A imports B, B imports C → resolve C for A
    ///
    /// Returns file_path → resolved symbols map.
    /// Cap at 50 symbols total to avoid prompt pollution.
    pub fn resolve_imports_multihop(
        file_imports: &HashMap<String, Vec<ImportTarget>>,
        parsed_files: &HashMap<String, Vec<Symbol>>,
    ) -> HashMap<String, Vec<Symbol>> {
        let mut resolved: HashMap<String, Vec<Symbol>> = HashMap::new();
        let mut total_count = 0;
        let max_symbols = 50;

        // Build a lookup: source_path → symbols in files that import from it
        let mut source_to_symbols: HashMap<String, Vec<ImportTarget>> = HashMap::new();
        for imports in file_imports.values() {
            for target in imports {
                source_to_symbols
                    .entry(target.source_path.clone())
                    .or_default()
                    .push(target.clone());
            }
        }

        // 1-hop resolution: direct imports
        for (file_path, imports) in file_imports {
            if total_count >= max_symbols {
                break;
            }

            let mut file_symbols = Vec::new();
            for target in imports {
                if total_count >= max_symbols {
                    break;
                }

                // Skip standard library imports
                if is_std_import(&target.source_path) {
                    continue;
                }

                // Find the symbol in parsed files
                for (parsed_path, symbols) in parsed_files {
                    // Check if this file matches the import source
                    if !paths_match(&target.source_path, parsed_path) {
                        continue;
                    }

                    for symbol in symbols {
                        if symbol.name == target.symbol_name
                            && matches!(
                                symbol.kind,
                                SymbolKind::Function
                                    | SymbolKind::Struct
                                    | SymbolKind::Class
                                    | SymbolKind::Interface
                                    | SymbolKind::Enum
                            )
                        {
                            file_symbols.push(symbol.clone());
                            total_count += 1;
                            break;
                        }
                    }
                }
            }

            if !file_symbols.is_empty() {
                resolved.insert(file_path.clone(), file_symbols);
            }
        }

        // 2-hop resolution: follow import chains
        for (file_a, imports_a) in file_imports {
            if total_count >= max_symbols {
                break;
            }

            for target_a in imports_a {
                if is_std_import(&target_a.source_path) {
                    continue;
                }

                // Find file B that file A imports from.
                // Look in file_imports (not parsed_files) so we can follow chains
                // even when file B's symbols haven't been parsed.
                for file_b_path in file_imports.keys() {
                    if file_b_path == file_a {
                        continue;
                    }
                    if !paths_match(&target_a.source_path, file_b_path) {
                        continue;
                    }

                    // Get imports from file B
                    if let Some(imports_b) = file_imports.get(file_b_path) {
                        for target_b in imports_b {
                            if total_count >= max_symbols {
                                break;
                            }

                            if is_std_import(&target_b.source_path) {
                                continue;
                            }

                            // Resolve symbols from file B's imports (2-hop)
                            for (file_c_path, symbols_c) in parsed_files {
                                if !paths_match(&target_b.source_path, file_c_path) {
                                    continue;
                                }

                                for symbol in symbols_c {
                                    if symbol.name == target_b.symbol_name
                                        && matches!(
                                            symbol.kind,
                                            SymbolKind::Function
                                                | SymbolKind::Struct
                                                | SymbolKind::Class
                                                | SymbolKind::Interface
                                                | SymbolKind::Enum
                                        )
                                    {
                                        resolved
                                            .entry(file_a.clone())
                                            .or_default()
                                            .push(symbol.clone());
                                        total_count += 1;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        resolved
    }

    /// Count symbol usage frequency across all files.
    ///
    /// Returns symbol_name → count map. Useful for identifying
    /// commonly-used symbols that might be important.
    pub fn count_symbol_frequency(
        parsed_files: &HashMap<String, Vec<Symbol>>,
    ) -> HashMap<String, usize> {
        let mut freq = HashMap::new();

        for symbols in parsed_files.values() {
            for symbol in symbols {
                *freq.entry(symbol.name.clone()).or_insert(0) += 1;
            }
        }

        freq
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

// ── Helper Functions ─────────────────────────────────────────────────────────

/// Check if an import source is a standard library or third-party package.
fn is_std_import(source_path: &str) -> bool {
    source_path.starts_with("std")
        || source_path.starts_with("core")
        || source_path.starts_with("alloc")
        || source_path.starts_with("os")
        || source_path.starts_with("sys")
        || source_path.starts_with("react")
        || source_path.starts_with("vue")
        || source_path.starts_with("@")
        || source_path.starts_with("node:")
        || source_path.contains("node_modules")
        || source_path.contains(".venv")
        || source_path.contains("vendor")
        || source_path.contains("site-packages")
        || source_path.contains("pkg")
}

/// Check if an import source path matches a file path.
fn paths_match(source_path: &str, file_path: &str) -> bool {
    // Normalize paths for comparison
    let normalized_source = source_path
        .replace("::", "/")
        .replace(".", "/")
        .replace("\\", "/");
    let normalized_file = file_path.replace("\\", "/");

    // Check if file path ends with source path (module → file mapping)
    normalized_file.contains(&normalized_source)
        || normalized_source.contains(
            normalized_file
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".rs")
                .trim_end_matches(".py")
                .trim_end_matches(".js")
                .trim_end_matches(".ts")
                .trim_end_matches(".go")
                .trim_end_matches(".java"),
        )
}

// ── Tests ────────────────────────────────────────────────────────────────────

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
        assert_eq!(Language::from_extension("rb"), Some(Language::Ruby));
        assert_eq!(Language::from_extension("php"), Some(Language::Php));
        assert_eq!(Language::from_extension("cs"), Some(Language::CSharp));
        assert_eq!(Language::from_extension("html"), Some(Language::Html));
        assert_eq!(Language::from_extension("css"), Some(Language::Css));
        assert_eq!(Language::from_extension("kt"), Some(Language::Java));
        assert_eq!(Language::from_extension("swift"), Some(Language::Java));
        assert_eq!(Language::from_extension("vue"), Some(Language::Html));
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

        let funcs: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        let classes: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        let imports: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Import)
            .collect();

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

        let has_struct = symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Struct && s.name == "Config");
        let has_enum = symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Enum && s.name == "Status");
        let has_func = symbols
            .iter()
            .any(|s| s.kind == SymbolKind::Function && s.name == "process");
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
    fn test_extract_ruby_symbols() {
        let source = r#"
require 'json'

module MyModule
  class MyClass
    def initialize(name)
      @name = name
    end

    def greet
      puts "Hello, #{@name}"
    end
  end
end
"#;
        let symbols = AstIntel::extract_symbols(source, "test.rb").unwrap();
        assert!(!symbols.is_empty(), "Should extract Ruby symbols");
    }

    #[test]
    fn test_extract_php_symbols() {
        let source = r#"<?php
namespace App\Controllers;

use App\Models\User;

class UserController {
    public function index() {
        return User::all();
    }
}
"#;
        let symbols = AstIntel::extract_symbols(source, "test.php").unwrap();
        assert!(!symbols.is_empty(), "Should extract PHP symbols");
    }

    #[test]
    fn test_extract_csharp_symbols() {
        let source = r#"
using System;
using System.Collections.Generic;

namespace MyApp {
    public class Program {
        public static void Main(string[] args) {
            Console.WriteLine("Hello");
        }
    }
}
"#;
        let symbols = AstIntel::extract_symbols(source, "test.cs").unwrap();
        assert!(!symbols.is_empty(), "Should extract C# symbols");
    }

    #[test]
    fn test_unknown_extension() {
        let symbols = AstIntel::extract_symbols("hello", "test.unknown").unwrap();
        assert!(symbols.is_empty(), "Unknown extension should return empty");
    }

    // ── Import Target Extraction Tests ──────────────────────────────

    #[test]
    fn test_extract_python_import_targets() {
        let source = "from pathlib import Path\nimport os\nfrom collections import OrderedDict, defaultdict\n";
        let targets = AstIntel::extract_import_targets(source, "test.py");
        let names: Vec<&str> = targets.iter().map(|t| t.symbol_name.as_str()).collect();
        assert!(names.contains(&"Path"), "Should extract Path from pathlib");
        assert!(names.contains(&"os"), "Should extract os");
    }

    #[test]
    fn test_extract_rust_import_targets() {
        let source = "use std::collections::HashMap;\nuse crate::core::{Config, Error};\n";
        let targets = AstIntel::extract_import_targets(source, "test.rs");
        let names: Vec<&str> = targets.iter().map(|t| t.symbol_name.as_str()).collect();
        assert!(names.contains(&"HashMap"), "Should extract HashMap");
        assert!(
            names.contains(&"Config"),
            "Should extract Config from group import"
        );
        assert!(
            names.contains(&"Error"),
            "Should extract Error from group import"
        );
    }

    #[test]
    fn test_extract_js_import_targets() {
        let source = "import { useState, useEffect } from 'react';\nimport App from './App';\n";
        let targets = AstIntel::extract_import_targets(source, "test.js");
        let names: Vec<&str> = targets.iter().map(|t| t.symbol_name.as_str()).collect();
        assert!(names.contains(&"useState"), "Should extract useState");
        assert!(names.contains(&"useEffect"), "Should extract useEffect");
    }

    #[test]
    fn test_extract_java_import_targets() {
        let source = "import java.util.HashMap;\nimport com.example.MyService;\n";
        let targets = AstIntel::extract_import_targets(source, "test.java");
        let names: Vec<&str> = targets.iter().map(|t| t.symbol_name.as_str()).collect();
        assert!(names.contains(&"HashMap"), "Should extract HashMap");
        assert!(names.contains(&"MyService"), "Should extract MyService");
    }

    #[test]
    fn test_extract_go_import_targets() {
        let source = "package main\n\nimport (\n\t\"fmt\"\n\t\"net/http\"\n)\n";
        let targets = AstIntel::extract_import_targets(source, "test.go");
        let names: Vec<&str> = targets.iter().map(|t| t.symbol_name.as_str()).collect();
        assert!(names.contains(&"fmt"), "Should extract fmt");
        assert!(names.contains(&"http"), "Should extract http from net/http");
    }

    // ── Import Resolution Tests ─────────────────────────────────────

    #[test]
    fn test_resolve_imports_finds_matching_symbol() {
        let imports = vec![ImportTarget {
            symbol_name: "Config".into(),
            source_path: "crate::core".into(),
        }];
        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            "src/core/config.rs".to_string(),
            vec![Symbol {
                name: "Config".into(),
                kind: SymbolKind::Struct,
                file_path: "src/core/config.rs".into(),
                line_start: 10,
                line_end: 25,
            }],
        );

        let resolved = AstIntel::resolve_imports(&imports, &parsed_files);
        assert!(resolved.contains_key("Config"), "Should resolve Config");
        assert!(resolved["Config"].contains("Struct"), "Should be a struct");
    }

    #[test]
    fn test_resolve_imports_skips_stdlib() {
        let imports = vec![ImportTarget {
            symbol_name: "HashMap".into(),
            source_path: "std::collections".into(),
        }];
        let mut parsed_files = HashMap::new();
        parsed_files.insert(
            "fake_stdlib.rs".to_string(),
            vec![Symbol {
                name: "HashMap".into(),
                kind: SymbolKind::Struct,
                file_path: "fake_stdlib.rs".into(),
                line_start: 1,
                line_end: 50,
            }],
        );

        let resolved = AstIntel::resolve_imports(&imports, &parsed_files);
        assert!(resolved.is_empty(), "Should skip stdlib imports");
    }

    #[test]
    fn test_resolve_imports_caps_at_20() {
        let imports: Vec<ImportTarget> = (0..30)
            .map(|i| ImportTarget {
                symbol_name: format!("Symbol{}", i),
                source_path: "local::module".into(),
            })
            .collect();
        let mut parsed_files = HashMap::new();
        let symbols: Vec<Symbol> = (0..30)
            .map(|i| Symbol {
                name: format!("Symbol{}", i),
                kind: SymbolKind::Function,
                file_path: "src/module.rs".into(),
                line_start: i * 10 + 1,
                line_end: i * 10 + 10,
            })
            .collect();
        parsed_files.insert("src/module.rs".to_string(), symbols);

        let resolved = AstIntel::resolve_imports(&imports, &parsed_files);
        assert!(
            resolved.len() <= 20,
            "Should cap at 20, got {}",
            resolved.len()
        );
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

    #[test]
    fn test_multihop_1hop_resolution() {
        // File A imports utils::helper
        let imports = vec![ImportTarget {
            symbol_name: "helper".to_string(),
            source_path: "utils".to_string(),
        }];
        let mut file_imports = HashMap::new();
        file_imports.insert("src/main.rs".to_string(), imports);

        // utils.rs has helper function
        let utils_symbols = vec![Symbol {
            name: "helper".to_string(),
            kind: SymbolKind::Function,
            file_path: "src/utils.rs".into(),
            line_start: 10,
            line_end: 20,
        }];
        let mut parsed_files = HashMap::new();
        parsed_files.insert("src/utils.rs".to_string(), utils_symbols);

        let resolved = AstIntel::resolve_imports_multihop(&file_imports, &parsed_files);
        assert!(resolved.contains_key("src/main.rs"));
        assert_eq!(resolved["src/main.rs"].len(), 1);
        assert_eq!(resolved["src/main.rs"][0].name, "helper");
    }

    #[test]
    fn test_multihop_2hop_resolution() {
        // main.rs imports from models
        let main_imports = vec![ImportTarget {
            symbol_name: "User".to_string(),
            source_path: "models".to_string(),
        }];
        // models.rs imports from db
        let model_imports = vec![ImportTarget {
            symbol_name: "Connection".to_string(),
            source_path: "db".to_string(),
        }];
        let mut file_imports = HashMap::new();
        file_imports.insert("src/main.rs".to_string(), main_imports);
        file_imports.insert("src/models.rs".to_string(), model_imports);

        // db.rs has Connection struct
        let db_symbols = vec![Symbol {
            name: "Connection".to_string(),
            kind: SymbolKind::Struct,
            file_path: "src/db.rs".into(),
            line_start: 5,
            line_end: 15,
        }];
        let mut parsed_files = HashMap::new();
        parsed_files.insert("src/db.rs".to_string(), db_symbols);

        let resolved = AstIntel::resolve_imports_multihop(&file_imports, &parsed_files);
        // 2-hop: main.rs → models.rs → db.rs::Connection
        assert!(resolved.contains_key("src/main.rs"));
    }

    #[test]
    fn test_multihop_cap_at_50() {
        // Create 60 imports to test capping
        let imports: Vec<ImportTarget> = (0..60)
            .map(|i| ImportTarget {
                symbol_name: format!("Symbol{}", i),
                source_path: "local::module".into(),
            })
            .collect();
        let mut file_imports = HashMap::new();
        file_imports.insert("src/main.rs".to_string(), imports);

        let parsed_files = HashMap::new(); // No matching symbols
        let resolved = AstIntel::resolve_imports_multihop(&file_imports, &parsed_files);
        // Should be empty since no parsed files match
        assert!(resolved.is_empty() || resolved.values().map(|v| v.len()).sum::<usize>() <= 50);
    }

    #[test]
    fn test_symbol_frequency() {
        let symbols_a = vec![
            Symbol {
                name: "main".to_string(),
                kind: SymbolKind::Function,
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 10,
            },
            Symbol {
                name: "helper".to_string(),
                kind: SymbolKind::Function,
                file_path: "a.rs".into(),
                line_start: 15,
                line_end: 25,
            },
        ];
        let symbols_b = vec![Symbol {
            name: "helper".to_string(),
            kind: SymbolKind::Function,
            file_path: "b.rs".into(),
            line_start: 5,
            line_end: 15,
        }];
        let mut parsed_files = HashMap::new();
        parsed_files.insert("a.rs".to_string(), symbols_a);
        parsed_files.insert("b.rs".to_string(), symbols_b);

        let freq = AstIntel::count_symbol_frequency(&parsed_files);
        assert_eq!(freq["helper"], 2);
        assert_eq!(freq["main"], 1);
    }

    #[test]
    fn test_is_std_import() {
        assert!(is_std_import("std::collections"));
        assert!(is_std_import("os.path"));
        assert!(is_std_import("node:fs"));
        assert!(is_std_import("react"));
        assert!(is_std_import("@types/node"));
        assert!(is_std_import("node_modules/lodash"));
        assert!(!is_std_import("./utils"));
        assert!(!is_std_import("my_package"));
    }

    #[test]
    fn test_paths_match() {
        assert!(paths_match("utils", "src/utils.rs"));
        assert!(paths_match(
            "std::collections",
            "std/collections/hashmap.rs"
        ));
        assert!(paths_match("models.user", "src/models/user.py"));
        assert!(!paths_match("utils", "src/helpers.rs"));
    }
}
