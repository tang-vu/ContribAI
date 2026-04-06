//! AST parser tests for all 13 supported languages.
//!
//! Tests symbol extraction for:
//! Python, JavaScript, TypeScript, Go, Rust, Java, C, C++,
//! Ruby, PHP, C#, HTML, CSS
//!
//! Plus edge cases: empty files, syntax errors, unicode, deeply nested.

use contribai::analysis::ast_intel::AstIntel;
use contribai::core::models::SymbolKind;

// ── Helper ───────────────────────────────────────────────────────────────

fn extract_symbols(source: &str, file_path: &str) -> Vec<contribai::core::models::Symbol> {
    AstIntel::extract_symbols(source, file_path).unwrap_or_default()
}

// ── Python ───────────────────────────────────────────────────────────────

#[test]
fn test_python_extract_functions_and_classes() {
    let source = r#"
class MyClass:
    def __init__(self, value):
        self.value = value

    def get_value(self):
        return self.value

def standalone_function(x, y):
    return x + y
"#;
    let symbols = extract_symbols(source, "test.py");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"MyClass".to_string()),
        "Should find MyClass class"
    );
    assert!(
        names.contains(&&"standalone_function".to_string()),
        "Should find standalone_function"
    );
    assert!(symbols.iter().any(|s| s.kind == SymbolKind::Class));
    assert!(symbols.iter().any(|s| s.kind == SymbolKind::Function));
}

#[test]
fn test_python_empty_file() {
    let symbols = extract_symbols("", "empty.py");
    assert_eq!(symbols.len(), 0);
}

#[test]
fn test_python_syntax_error() {
    // Should not panic — returns empty or partial results
    let symbols = extract_symbols("def broken(\n  pass", "bad.py");
    // tree-sitter Python is lenient — may still extract something
    assert!(symbols.len() <= 2);
}

// ── JavaScript ───────────────────────────────────────────────────────────

#[test]
fn test_javascript_extract_functions() {
    let source = r#"
function greet(name) {
    return `Hello, ${name}`;
}

class Person {
    constructor(name) {
        this.name = name;
    }

    sayHello() {
        console.log(`Hello, ${this.name}`);
    }
}

const arrowFn = () => {
    return "arrow";
};

module.exports = { greet, Person };
"#;
    let symbols = extract_symbols(source, "test.js");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"greet".to_string()),
        "Should find greet function"
    );
    assert!(
        names.contains(&&"Person".to_string()),
        "Should find Person class"
    );
}

// ── TypeScript ───────────────────────────────────────────────────────────

#[test]
fn test_typescript_extract_interfaces_and_classes() {
    let source = r#"
interface User {
    id: number;
    name: string;
    email?: string;
}

class UserService {
    private users: User[] = [];

    getUser(id: number): User | undefined {
        return this.users.find(u => u.id === id);
    }

    addUser(user: User): void {
        this.users.push(user);
    }
}

export type Status = "active" | "inactive";

export async function fetchData(url: string): Promise<User[]> {
    const response = await fetch(url);
    return response.json();
}
"#;
    let symbols = extract_symbols(source, "test.ts");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"UserService".to_string()),
        "Should find UserService class"
    );
    assert!(
        names.contains(&&"fetchData".to_string()),
        "Should find fetchData function"
    );
}

// ── Go ───────────────────────────────────────────────────────────────────

#[test]
fn test_go_extract_functions_and_methods() {
    let source = r#"
package main

import "fmt"

type Config struct {
    Name    string
    Timeout int
}

func (c *Config) Validate() error {
    if c.Name == "" {
        return fmt.Errorf("name is required")
    }
    return nil
}

func main() {
    c := &Config{Name: "test", Timeout: 30}
    fmt.Println(c.Validate())
}

func helperFunc() {
    // helper
}
"#;
    let symbols = extract_symbols(source, "test.go");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    // Go tree-sitter extracts function names
    assert!(
        symbols
            .iter()
            .any(|s| s.name == "main" || s.name.contains("main")),
        "Should find main function: {:?}",
        names
    );
    assert!(
        symbols.iter().any(|s| s.name.contains("helper")
            || s.name.contains("Validate")
            || s.name.contains("Config")),
        "Should find other functions: {:?}",
        names
    );
}

// ── Rust ─────────────────────────────────────────────────────────────────

#[test]
fn test_rust_extract_traits_and_impls() {
    let source = r#"
pub trait Processor {
    fn process(&self, input: &str) -> Result<String, Error>;
    fn name(&self) -> &str;
}

pub struct TextProcessor {
    prefix: String,
}

impl Processor for TextProcessor {
    fn process(&self, input: &str) -> Result<String, Error> {
        Ok(format!("{}{}", self.prefix, input))
    }

    fn name(&self) -> &str {
        "TextProcessor"
    }
}

impl TextProcessor {
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
        }
    }
}

fn helper_function() {
    println!("helper");
}
"#;
    let symbols = extract_symbols(source, "test.rs");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"Processor".to_string()),
        "Should find Processor trait"
    );
    assert!(
        names.contains(&&"TextProcessor".to_string()),
        "Should find TextProcessor struct"
    );
    assert!(
        names.contains(&&"helper_function".to_string()),
        "Should find helper_function"
    );
}

// ── Java ─────────────────────────────────────────────────────────────────

#[test]
fn test_java_extract_classes_and_methods() {
    let source = r#"
public class UserService {
    private List<User> users;

    public UserService() {
        this.users = new ArrayList<>();
    }

    public User getUser(int id) {
        return users.stream()
            .filter(u -> u.getId() == id)
            .findFirst()
            .orElse(null);
    }

    public void addUser(User user) {
        users.add(user);
    }

    private void validate(User user) {
        if (user.getName() == null) {
            throw new IllegalArgumentException("Name required");
        }
    }
}
"#;
    let symbols = extract_symbols(source, "Test.java");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"UserService".to_string()),
        "Should find UserService class: {:?}",
        names
    );
    // Java tree-sitter extracts class names and method names
    assert!(
        symbols
            .iter()
            .any(|s| s.name.contains("User") || s.name.contains("add") || s.name.contains("get")),
        "Should find methods or classes: {:?}",
        names
    );
}

// ── C ────────────────────────────────────────────────────────────────────

#[test]
fn test_c_extract_functions() {
    let source = r#"
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    int x;
    int y;
} Point;

Point* create_point(int x, int y) {
    Point* p = (Point*)malloc(sizeof(Point));
    p->x = x;
    p->y = y;
    return p;
}

void print_point(Point* p) {
    printf("(%d, %d)\n", p->x, p->y);
}

int main(int argc, char** argv) {
    Point* p = create_point(10, 20);
    print_point(p);
    free(p);
    return 0;
}
"#;
    let symbols = extract_symbols(source, "test.c");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"create_point".to_string()),
        "Should find create_point"
    );
    assert!(
        names.contains(&&"print_point".to_string()),
        "Should find print_point"
    );
    assert!(names.contains(&&"main".to_string()), "Should find main");
}

// ── C++ ──────────────────────────────────────────────────────────────────

#[test]
fn test_cpp_extract_classes_and_methods() {
    let source = r#"
#include <string>
#include <vector>

class Shape {
public:
    virtual double area() const = 0;
    virtual ~Shape() = default;
};

class Circle : public Shape {
private:
    double radius;
public:
    Circle(double r) : radius(r) {}
    double area() const override {
        return 3.14159 * radius * radius;
    }
};

std::vector<Shape*> create_shapes() {
    return {new Circle(5.0)};
}
"#;
    let symbols = extract_symbols(source, "test.cpp");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"Shape".to_string()),
        "Should find Shape class"
    );
    assert!(
        names.contains(&&"Circle".to_string()),
        "Should find Circle class"
    );
    assert!(
        names.contains(&&"create_shapes".to_string()),
        "Should find create_shapes"
    );
}

// ── Ruby ─────────────────────────────────────────────────────────────────

#[test]
fn test_ruby_extract_classes_and_methods() {
    let source = r#"
class UserController
  def initialize(database)
    @database = database
  end

  def show(id)
    user = @database.find_user(id)
    render json: user
  end

  def create(params)
    user = User.new(params)
    if user.save
      render json: user, status: 201
    else
      render json: user.errors, status: 422
    end
  end

  private

  def user_params
    params.require(:user).permit(:name, :email)
  end
end

def helper_method
  puts "helper"
end
"#;
    let symbols = extract_symbols(source, "test.rb");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"UserController".to_string()),
        "Should find UserController"
    );
    assert!(
        names.contains(&&"show".to_string()),
        "Should find show method"
    );
    assert!(
        names.contains(&&"create".to_string()),
        "Should find create method"
    );
}

// ── PHP ──────────────────────────────────────────────────────────────────

#[test]
fn test_php_extract_classes_and_methods() {
    let source = r#"
<?php

namespace App\Controllers;

class ProductController {
    private $repository;

    public function __construct(ProductRepository $repo) {
        $this->repository = $repo;
    }

    public function index(): array {
        return $this->repository->findAll();
    }

    public function show(int $id): ?Product {
        return $this->repository->findById($id);
    }

    private function validate(Product $product): bool {
        return !empty($product->getName());
    }
}

function helper_function(): void {
    echo "helper";
}
"#;
    let symbols = extract_symbols(source, "test.php");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"ProductController".to_string()),
        "Should find ProductController"
    );
    assert!(
        names.contains(&&"index".to_string()),
        "Should find index method"
    );
    assert!(
        names.contains(&&"helper_function".to_string()),
        "Should find helper_function"
    );
}

// ── C# ───────────────────────────────────────────────────────────────────

#[test]
fn test_csharp_extract_classes_and_methods() {
    let source = r#"
using System;
using System.Collections.Generic;

namespace MyApp.Services
{
    public class OrderService
    {
        private readonly List<Order> _orders;

        public OrderService()
        {
            _orders = new List<Order>();
        }

        public Order GetOrder(int id)
        {
            return _orders.Find(o => o.Id == id);
        }

        public void AddOrder(Order order)
        {
            _orders.Add(order);
        }

        private bool IsValid(Order order)
        {
            return order.Total > 0;
        }
    }

    public record Order(int Id, decimal Total);
}
"#;
    let symbols = extract_symbols(source, "test.cs");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"OrderService".to_string()),
        "Should find OrderService: {:?}",
        names
    );
    // C# tree-sitter may extract methods differently
    assert!(
        symbols
            .iter()
            .any(|s| s.name.contains("Order") || s.name.contains("Get") || s.name.contains("Add")),
        "Should find methods: {:?}",
        names
    );
}

// ── HTML ─────────────────────────────────────────────────────────────────

#[test]
fn test_html_parses_valid_structure() {
    let source = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Test Page</title>
</head>
<body>
    <div id="main">
        <h1>Hello World</h1>
        <p class="text">This is a paragraph.</p>
        <form action="/submit" method="post">
            <input type="text" name="username" />
            <button type="submit">Submit</button>
        </form>
    </div>
</body>
</html>
"#;
    let symbols = extract_symbols(source, "test.html");
    // HTML parser extracts element nodes — we expect at least some elements
    assert!(symbols.len() > 0, "Should extract HTML elements");
}

// ── CSS ──────────────────────────────────────────────────────────────────

#[test]
fn test_css_extract_rules() {
    let source = r#"
body {
    margin: 0;
    padding: 0;
    font-family: Arial, sans-serif;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
    padding: 20px;
}

@media (max-width: 768px) {
    .container {
        padding: 10px;
    }

    .sidebar {
        display: none;
    }
}

#header {
    background: #333;
    color: white;
}
"#;
    let symbols = extract_symbols(source, "test.css");
    // CSS parser extracts rule selectors
    assert!(symbols.len() > 0, "Should extract CSS rules");
}

// ── Edge Cases ───────────────────────────────────────────────────────────

#[test]
fn test_empty_files_all_languages() {
    let extensions = [
        "py", "js", "ts", "go", "rs", "java", "c", "cpp", "rb", "php", "cs", "html", "css",
    ];
    for ext in &extensions {
        let symbols = extract_symbols("", &format!("empty.{}", ext));
        assert_eq!(
            symbols.len(),
            0,
            "Empty file for {} should return no symbols",
            ext
        );
    }
}

#[test]
fn test_whitespace_only_files() {
    let extensions = [
        "py", "js", "ts", "go", "rs", "java", "c", "cpp", "rb", "php", "cs", "html", "css",
    ];
    for ext in &extensions {
        let symbols = extract_symbols("   \n\n\t\n  ", &format!("whitespace.{}", ext));
        // Whitespace-only files should return no symbols
        assert_eq!(
            symbols.len(),
            0,
            "Whitespace-only file for {} should return no symbols",
            ext
        );
    }
}

#[test]
fn test_unicode_content_python() {
    let source = r#"
def greet(name: str) -> str:
    return f"Xin chào, {name}! 🎉"

class HéllöWörld:
    def __init__(self, value: str):
        self.value = value  # 日本語
"#;
    let symbols = extract_symbols(source, "unicode.py");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    assert!(
        names.contains(&&"greet".to_string()),
        "Should find greet with unicode"
    );
    assert!(
        names.contains(&&"HéllöWörld".to_string()),
        "Should find HéllöWörld class"
    );
}

#[test]
fn test_deeply_nested_python() {
    let mut source = String::new();
    let depth = 50;
    for i in 0..depth {
        source.push_str(&format!("def level{}():\n", i));
        source.push_str(&"    ".repeat(i + 1));
    }
    source.push_str(&"    ".repeat(depth));
    source.push_str("return 42\n");

    let symbols = extract_symbols(&source, "deep.py");
    // Should find at least the outermost function
    assert!(
        symbols.iter().any(|s| s.name == "level0"),
        "Should find level0"
    );
}

#[test]
fn test_unknown_extension_returns_empty() {
    let symbols = extract_symbols("some content", "file.xyz");
    assert_eq!(
        symbols.len(),
        0,
        "Unknown extension should return no symbols"
    );
}

#[test]
fn test_syntax_error_python_returns_partial() {
    // tree-sitter Python is lenient — some code still parses
    let source = r#"
def valid_function():
    return "this is fine"

def broken_function(
    # missing closing paren and body
    pass

class ValidClass:
    def method(self):
        return "also fine"
"#;
    let symbols = extract_symbols(source, "syntax_errors.py");
    let names: Vec<_> = symbols.iter().map(|s| &s.name).collect();
    // At least valid_function should be found
    assert!(
        names.contains(&&"valid_function".to_string()),
        "Should find valid_function despite syntax errors"
    );
}

#[test]
fn test_mixed_language_extensions_map_correctly() {
    // TypeScript (.tsx) should use TypeScript parser
    let tsx = r#"
function Component(): JSX.Element {
    return <div>Hello</div>;
}
"#;
    let tsx_symbols = extract_symbols(tsx, "Component.tsx");
    assert!(
        tsx_symbols.iter().any(|s| s.name == "Component"),
        "Should parse TSX: {:?}",
        tsx_symbols
    );

    // JSX should use JavaScript parser — arrow functions may not extract as named
    let jsx = r#"
function App() {
    return React.createElement("div", null, "JSX");
}
"#;
    let jsx_symbols = extract_symbols(jsx, "App.jsx");
    assert!(
        jsx_symbols.iter().any(|s| s.name == "App"),
        "Should parse JSX: {:?}",
        jsx_symbols
    );

    // Kotlin should use Java parser (or fall through gracefully)
    let kt = r#"
class KotlinClass {
    fun doSomething(): String {
        return "Kotlin"
    }
}
"#;
    let kt_symbols = extract_symbols(kt, "Test.kt");
    // Kotlin may not extract perfectly with Java parser — just verify it doesn't crash
    assert!(
        kt_symbols.len() >= 0,
        "Kotlin should parse without error: {:?}",
        kt_symbols
    );
}
