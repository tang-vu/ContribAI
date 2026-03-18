"""Multi-language analysis rules and prompts.

Extends the core analysis system with language-specific
security patterns, best practices, and code quality rules
for JavaScript/TypeScript, Go, and Rust.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

logger = logging.getLogger(__name__)


@dataclass
class LanguageRule:
    """A language-specific analysis rule."""

    language: str
    category: str  # security | code_quality | docs | performance
    name: str
    description: str
    pattern: str  # what to look for
    severity: str = "medium"
    fix_hint: str = ""


# ── JavaScript / TypeScript ──────────────────────────────


JS_TS_RULES: list[LanguageRule] = [
    LanguageRule(
        language="javascript",
        category="security",
        name="eval-usage",
        description="Use of eval() is a security risk",
        pattern="eval(",
        severity="critical",
        fix_hint="Replace eval() with JSON.parse() or a safe alternative",
    ),
    LanguageRule(
        language="javascript",
        category="security",
        name="innerHTML-xss",
        description="innerHTML can lead to XSS attacks",
        pattern="innerHTML",
        severity="high",
        fix_hint="Use textContent or a sanitization library like DOMPurify",
    ),
    LanguageRule(
        language="javascript",
        category="security",
        name="no-prototype-pollution",
        description="Prototype pollution via __proto__",
        pattern="__proto__",
        severity="critical",
        fix_hint="Use Object.create(null) or validate input keys",
    ),
    LanguageRule(
        language="typescript",
        category="code_quality",
        name="no-any-type",
        description="Avoid 'any' type - defeats TypeScript's purpose",
        pattern=": any",
        severity="medium",
        fix_hint="Use specific types or 'unknown' with type guards",
    ),
    LanguageRule(
        language="javascript",
        category="code_quality",
        name="no-var",
        description="Use const/let instead of var",
        pattern="var ",
        severity="low",
        fix_hint="Replace 'var' with 'const' (immutable) or 'let' (mutable)",
    ),
    LanguageRule(
        language="javascript",
        category="performance",
        name="no-sync-fs",
        description="Synchronous fs operations block the event loop",
        pattern="readFileSync",
        severity="medium",
        fix_hint="Use async fs.readFile() or fs.promises.readFile()",
    ),
    LanguageRule(
        language="javascript",
        category="security",
        name="no-hardcoded-jwt-secret",
        description="Hardcoded JWT secret key",
        pattern="jwt.sign(",
        severity="high",
        fix_hint="Use environment variables for JWT secrets",
    ),
]


# ── Go ───────────────────────────────────────────────


GO_RULES: list[LanguageRule] = [
    LanguageRule(
        language="go",
        category="security",
        name="sql-injection",
        description="Potential SQL injection via string formatting",
        pattern='fmt.Sprintf("SELECT',
        severity="critical",
        fix_hint="Use parameterized queries with db.Query(sql, args...)",
    ),
    LanguageRule(
        language="go",
        category="code_quality",
        name="unchecked-error",
        description="Ignoring error return values",
        pattern="_ = ",
        severity="medium",
        fix_hint="Handle errors explicitly: if err != nil { return err }",
    ),
    LanguageRule(
        language="go",
        category="code_quality",
        name="defer-in-loop",
        description="Defer inside loop can cause resource leaks",
        pattern="defer ",
        severity="medium",
        fix_hint="Move defer outside the loop or use a wrapper function",
    ),
    LanguageRule(
        language="go",
        category="security",
        name="tls-insecure-skip",
        description="TLS verification disabled",
        pattern="InsecureSkipVerify: true",
        severity="critical",
        fix_hint="Remove InsecureSkipVerify or use proper CA certificates",
    ),
    LanguageRule(
        language="go",
        category="performance",
        name="goroutine-leak",
        description="Goroutine without context cancellation",
        pattern="go func()",
        severity="medium",
        fix_hint="Use context.WithCancel and select for graceful shutdown",
    ),
    LanguageRule(
        language="go",
        category="docs",
        name="missing-package-doc",
        description="Package missing documentation comment",
        pattern="package ",
        severity="low",
        fix_hint="Add // Package <name> ... comment above package declaration",
    ),
]


# ── Rust ──────────────────────────────────────────────


RUST_RULES: list[LanguageRule] = [
    LanguageRule(
        language="rust",
        category="security",
        name="unsafe-block",
        description="Unsafe block bypasses Rust's safety guarantees",
        pattern="unsafe {",
        severity="high",
        fix_hint="Document why unsafe is necessary; use safe alternatives when possible",
    ),
    LanguageRule(
        language="rust",
        category="code_quality",
        name="unwrap-panic",
        description="unwrap() panics on None/Err — use ? operator",
        pattern=".unwrap()",
        severity="medium",
        fix_hint="Use .unwrap_or(), .unwrap_or_default(), or the ? operator",
    ),
    LanguageRule(
        language="rust",
        category="code_quality",
        name="expect-panic",
        description="expect() panics with message — use ? in libraries",
        pattern=".expect(",
        severity="low",
        fix_hint="Use ? operator in library code; expect() is OK in main/tests",
    ),
    LanguageRule(
        language="rust",
        category="performance",
        name="clone-heavy",
        description="Excessive .clone() may indicate ownership issues",
        pattern=".clone()",
        severity="low",
        fix_hint="Use references (&T) or Cow<T> to avoid unnecessary cloning",
    ),
    LanguageRule(
        language="rust",
        category="security",
        name="raw-pointer-deref",
        description="Raw pointer dereference is unsafe",
        pattern="*const ",
        severity="high",
        fix_hint="Use safe references (&T, &mut T) instead of raw pointers",
    ),
    LanguageRule(
        language="rust",
        category="code_quality",
        name="todo-macro",
        description="todo!() macro will panic at runtime",
        pattern="todo!()",
        severity="medium",
        fix_hint="Implement the missing functionality or use unimplemented!()",
    ),
]


# ── Registry ─────────────────────────────────────────


ALL_RULES = JS_TS_RULES + GO_RULES + RUST_RULES

RULES_BY_LANGUAGE: dict[str, list[LanguageRule]] = {}
for _rule in ALL_RULES:
    lang = _rule.language
    if lang not in RULES_BY_LANGUAGE:
        RULES_BY_LANGUAGE[lang] = []
    RULES_BY_LANGUAGE[lang].append(_rule)


def get_rules_for_language(
    language: str,
) -> list[LanguageRule]:
    """Get analysis rules for a specific language."""
    lang = language.lower()
    rules = RULES_BY_LANGUAGE.get(lang, [])
    # TypeScript inherits JavaScript rules
    if lang == "typescript":
        rules = rules + RULES_BY_LANGUAGE.get("javascript", [])
    return rules


def get_analysis_prompt(
    language: str,
    file_content: str,
    file_path: str,
) -> str:
    """Generate a language-specific analysis prompt."""
    rules = get_rules_for_language(language)
    if not rules:
        return ""

    rule_descriptions = "\n".join(
        f"- [{r.severity.upper()}] {r.name}: {r.description} (look for: {r.pattern})" for r in rules
    )

    return f"""Analyze this {language} file for issues:

File: {file_path}

Language-specific rules to check:
{rule_descriptions}

For each issue found, provide:
1. Rule name that triggered
2. Line number(s)
3. Description of the problem
4. Suggested fix

Code:
```
{file_content[:5000]}
```
"""


def get_supported_languages() -> list[str]:
    """Get list of languages with specialized rules."""
    return sorted(set(RULES_BY_LANGUAGE.keys()))
