//! # Syntax Highlighting Engine
//!
//! Provides syntax highlighting for S-CODE editor.
//! Supports multiple languages with extensible grammar definitions.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// =============================================================================
// Token Types
// =============================================================================

/// Token type for syntax highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    /// Plain text.
    Text,
    /// Keyword (if, for, while, etc.).
    Keyword,
    /// Built-in type (int, string, etc.).
    Type,
    /// Function name.
    Function,
    /// Variable name.
    Variable,
    /// String literal.
    String,
    /// Character literal.
    Char,
    /// Number literal.
    Number,
    /// Comment.
    Comment,
    /// Documentation comment.
    DocComment,
    /// Operator (+, -, *, etc.).
    Operator,
    /// Punctuation (, ; : etc.).
    Punctuation,
    /// Preprocessor directive.
    Preprocessor,
    /// Macro.
    Macro,
    /// Attribute/annotation.
    Attribute,
    /// Namespace/module.
    Namespace,
    /// Constant.
    Constant,
    /// Error/invalid.
    Error,
}

/// A highlighted token.
#[derive(Debug, Clone)]
pub struct Token {
    /// Token type.
    pub token_type: TokenType,
    /// Start column (0-indexed).
    pub start: usize,
    /// End column (exclusive).
    pub end: usize,
}

impl Token {
    pub fn new(token_type: TokenType, start: usize, end: usize) -> Self {
        Self { token_type, start, end }
    }
}

// =============================================================================
// Color Theme
// =============================================================================

/// RGB color.
#[derive(Debug, Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Color theme for syntax highlighting.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Background color.
    pub background: Rgb,
    /// Foreground (default text) color.
    pub foreground: Rgb,
    /// Token colors.
    pub colors: BTreeMap<u8, Rgb>, // TokenType as u8 -> color
}

impl Theme {
    /// Dark theme (default).
    pub fn dark() -> Self {
        let mut colors = BTreeMap::new();
        colors.insert(TokenType::Text as u8, Rgb(212, 212, 212));
        colors.insert(TokenType::Keyword as u8, Rgb(197, 134, 192));
        colors.insert(TokenType::Type as u8, Rgb(78, 201, 176));
        colors.insert(TokenType::Function as u8, Rgb(220, 220, 170));
        colors.insert(TokenType::Variable as u8, Rgb(156, 220, 254));
        colors.insert(TokenType::String as u8, Rgb(206, 145, 120));
        colors.insert(TokenType::Char as u8, Rgb(206, 145, 120));
        colors.insert(TokenType::Number as u8, Rgb(181, 206, 168));
        colors.insert(TokenType::Comment as u8, Rgb(106, 153, 85));
        colors.insert(TokenType::DocComment as u8, Rgb(86, 156, 214));
        colors.insert(TokenType::Operator as u8, Rgb(212, 212, 212));
        colors.insert(TokenType::Punctuation as u8, Rgb(212, 212, 212));
        colors.insert(TokenType::Preprocessor as u8, Rgb(155, 155, 155));
        colors.insert(TokenType::Macro as u8, Rgb(79, 193, 255));
        colors.insert(TokenType::Attribute as u8, Rgb(156, 220, 254));
        colors.insert(TokenType::Namespace as u8, Rgb(78, 201, 176));
        colors.insert(TokenType::Constant as u8, Rgb(79, 193, 255));
        colors.insert(TokenType::Error as u8, Rgb(244, 71, 71));

        Self {
            background: Rgb(30, 30, 30),
            foreground: Rgb(212, 212, 212),
            colors,
        }
    }

    /// Light theme.
    pub fn light() -> Self {
        let mut colors = BTreeMap::new();
        colors.insert(TokenType::Text as u8, Rgb(0, 0, 0));
        colors.insert(TokenType::Keyword as u8, Rgb(175, 0, 219));
        colors.insert(TokenType::Type as u8, Rgb(38, 127, 153));
        colors.insert(TokenType::Function as u8, Rgb(121, 94, 38));
        colors.insert(TokenType::Variable as u8, Rgb(0, 16, 128));
        colors.insert(TokenType::String as u8, Rgb(163, 21, 21));
        colors.insert(TokenType::Char as u8, Rgb(163, 21, 21));
        colors.insert(TokenType::Number as u8, Rgb(9, 134, 88));
        colors.insert(TokenType::Comment as u8, Rgb(0, 128, 0));
        colors.insert(TokenType::DocComment as u8, Rgb(0, 0, 255));
        colors.insert(TokenType::Operator as u8, Rgb(0, 0, 0));
        colors.insert(TokenType::Punctuation as u8, Rgb(0, 0, 0));
        colors.insert(TokenType::Preprocessor as u8, Rgb(128, 128, 128));
        colors.insert(TokenType::Macro as u8, Rgb(0, 112, 193));
        colors.insert(TokenType::Attribute as u8, Rgb(0, 16, 128));
        colors.insert(TokenType::Namespace as u8, Rgb(38, 127, 153));
        colors.insert(TokenType::Constant as u8, Rgb(0, 112, 193));
        colors.insert(TokenType::Error as u8, Rgb(255, 0, 0));

        Self {
            background: Rgb(255, 255, 255),
            foreground: Rgb(0, 0, 0),
            colors,
        }
    }

    /// Gets color for token type.
    pub fn get_color(&self, token_type: TokenType) -> Rgb {
        self.colors.get(&(token_type as u8)).copied().unwrap_or(self.foreground)
    }
}

// =============================================================================
// Language Definition
// =============================================================================

/// Language definition for syntax highlighting.
#[derive(Debug, Clone)]
pub struct Language {
    /// Language name.
    pub name: String,
    /// File extensions.
    pub extensions: Vec<String>,
    /// Keywords.
    pub keywords: Vec<String>,
    /// Type keywords.
    pub types: Vec<String>,
    /// Built-in functions/values.
    pub builtins: Vec<String>,
    /// Single-line comment prefix.
    pub line_comment: Option<String>,
    /// Block comment (start, end).
    pub block_comment: Option<(String, String)>,
    /// String delimiters.
    pub string_delimiters: Vec<char>,
    /// Char delimiter.
    pub char_delimiter: Option<char>,
}

impl Language {
    /// Rust language definition.
    pub fn rust() -> Self {
        Self {
            name: "Rust".into(),
            extensions: vec!["rs".into()],
            keywords: vec![
                "as", "async", "await", "break", "const", "continue", "crate", "dyn",
                "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
                "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
                "self", "Self", "static", "struct", "super", "trait", "true", "type",
                "unsafe", "use", "where", "while", "macro_rules",
            ].into_iter().map(String::from).collect(),
            types: vec![
                "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize",
                "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64",
                "String", "Vec", "Option", "Result", "Box", "Rc", "Arc",
            ].into_iter().map(String::from).collect(),
            builtins: vec![
                "Some", "None", "Ok", "Err", "println", "print", "eprintln", "eprint",
                "format", "vec", "panic", "assert", "assert_eq", "debug_assert",
            ].into_iter().map(String::from).collect(),
            line_comment: Some("//".into()),
            block_comment: Some(("/*".into(), "*/".into())),
            string_delimiters: vec!['"'],
            char_delimiter: Some('\''),
        }
    }

    /// JavaScript language definition.
    pub fn javascript() -> Self {
        Self {
            name: "JavaScript".into(),
            extensions: vec!["js".into(), "mjs".into(), "jsx".into()],
            keywords: vec![
                "async", "await", "break", "case", "catch", "class", "const", "continue",
                "debugger", "default", "delete", "do", "else", "export", "extends",
                "finally", "for", "function", "if", "import", "in", "instanceof", "let",
                "new", "return", "static", "super", "switch", "this", "throw", "try",
                "typeof", "var", "void", "while", "with", "yield",
            ].into_iter().map(String::from).collect(),
            types: vec![
                "Array", "Boolean", "Date", "Error", "Function", "JSON", "Map", "Math",
                "Number", "Object", "Promise", "RegExp", "Set", "String", "Symbol",
            ].into_iter().map(String::from).collect(),
            builtins: vec![
                "console", "document", "window", "global", "process", "module", "exports",
                "require", "setTimeout", "setInterval", "fetch", "undefined", "null",
                "true", "false", "NaN", "Infinity",
            ].into_iter().map(String::from).collect(),
            line_comment: Some("//".into()),
            block_comment: Some(("/*".into(), "*/".into())),
            string_delimiters: vec!['"', '\'', '`'],
            char_delimiter: None,
        }
    }

    /// Python language definition.
    pub fn python() -> Self {
        Self {
            name: "Python".into(),
            extensions: vec!["py".into(), "pyw".into()],
            keywords: vec![
                "False", "None", "True", "and", "as", "assert", "async", "await",
                "break", "class", "continue", "def", "del", "elif", "else", "except",
                "finally", "for", "from", "global", "if", "import", "in", "is",
                "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try",
                "while", "with", "yield",
            ].into_iter().map(String::from).collect(),
            types: vec![
                "int", "float", "str", "bool", "list", "dict", "set", "tuple",
                "bytes", "bytearray", "object", "type",
            ].into_iter().map(String::from).collect(),
            builtins: vec![
                "print", "len", "range", "open", "input", "type", "isinstance", "hasattr",
                "getattr", "setattr", "dir", "help", "id", "hash", "abs", "round",
                "min", "max", "sum", "sorted", "reversed", "enumerate", "zip", "map",
                "filter", "all", "any",
            ].into_iter().map(String::from).collect(),
            line_comment: Some("#".into()),
            block_comment: None,
            string_delimiters: vec!['"', '\''],
            char_delimiter: None,
        }
    }

    /// C language definition.
    pub fn c() -> Self {
        Self {
            name: "C".into(),
            extensions: vec!["c".into(), "h".into()],
            keywords: vec![
                "auto", "break", "case", "char", "const", "continue", "default", "do",
                "double", "else", "enum", "extern", "float", "for", "goto", "if",
                "inline", "int", "long", "register", "restrict", "return", "short",
                "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
                "unsigned", "void", "volatile", "while", "_Bool", "_Complex", "_Imaginary",
            ].into_iter().map(String::from).collect(),
            types: vec![
                "size_t", "ptrdiff_t", "intptr_t", "uintptr_t", "int8_t", "int16_t",
                "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t",
                "FILE", "NULL",
            ].into_iter().map(String::from).collect(),
            builtins: vec![
                "printf", "scanf", "malloc", "free", "memcpy", "memset", "strlen",
                "strcpy", "strcat", "strcmp", "fopen", "fclose", "fread", "fwrite",
            ].into_iter().map(String::from).collect(),
            line_comment: Some("//".into()),
            block_comment: Some(("/*".into(), "*/".into())),
            string_delimiters: vec!['"'],
            char_delimiter: Some('\''),
        }
    }
}

// =============================================================================
// Highlighter
// =============================================================================

/// Lexer state for multi-line constructs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerState {
    /// Normal state.
    Normal,
    /// Inside a string.
    InString(char),
    /// Inside a block comment.
    InBlockComment,
    /// Inside a raw string (Rust).
    InRawString(u8), // Number of # symbols
}

/// Syntax highlighter.
pub struct Highlighter {
    /// Language definition.
    language: Language,
    /// Current lexer state.
    state: LexerState,
}

impl Highlighter {
    /// Creates a new highlighter for a language.
    pub fn new(language: Language) -> Self {
        Self {
            language,
            state: LexerState::Normal,
        }
    }

    /// Gets the language.
    pub fn language(&self) -> &Language {
        &self.language
    }

    /// Resets the lexer state.
    pub fn reset(&mut self) {
        self.state = LexerState::Normal;
    }

    /// Highlights a line, returning tokens.
    pub fn highlight_line(&mut self, line: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            match self.state {
                LexerState::Normal => {
                    // Skip whitespace
                    if chars[i].is_whitespace() {
                        i += 1;
                        continue;
                    }

                    // Check for line comment
                    if let Some(ref lc) = self.language.line_comment {
                        if line[i..].starts_with(lc) {
                            let is_doc = line[i..].starts_with("///") || line[i..].starts_with("//!");
                            tokens.push(Token::new(
                                if is_doc { TokenType::DocComment } else { TokenType::Comment },
                                i,
                                chars.len(),
                            ));
                            return tokens;
                        }
                    }

                    // Check for block comment start
                    if let Some((ref start, _)) = self.language.block_comment {
                        if line[i..].starts_with(start) {
                            self.state = LexerState::InBlockComment;
                            let start_pos = i;
                            i += start.len();
                            
                            // Look for end on same line
                            if let Some((_, ref end)) = self.language.block_comment {
                                if let Some(pos) = line[i..].find(end.as_str()) {
                                    tokens.push(Token::new(TokenType::Comment, start_pos, i + pos + end.len()));
                                    i += pos + end.len();
                                    self.state = LexerState::Normal;
                                    continue;
                                }
                            }
                            tokens.push(Token::new(TokenType::Comment, start_pos, chars.len()));
                            return tokens;
                        }
                    }

                    // Check for string
                    if self.language.string_delimiters.contains(&chars[i]) {
                        let delim = chars[i];
                        let start_pos = i;
                        i += 1;
                        
                        while i < chars.len() {
                            if chars[i] == '\\' && i + 1 < chars.len() {
                                i += 2; // Skip escape sequence
                            } else if chars[i] == delim {
                                i += 1;
                                break;
                            } else {
                                i += 1;
                            }
                        }
                        
                        tokens.push(Token::new(TokenType::String, start_pos, i));
                        continue;
                    }

                    // Check for char literal
                    if let Some(delim) = self.language.char_delimiter {
                        if chars[i] == delim {
                            let start_pos = i;
                            i += 1;
                            
                            while i < chars.len() && chars[i] != delim {
                                if chars[i] == '\\' && i + 1 < chars.len() {
                                    i += 2;
                                } else {
                                    i += 1;
                                }
                            }
                            if i < chars.len() {
                                i += 1;
                            }
                            
                            tokens.push(Token::new(TokenType::Char, start_pos, i));
                            continue;
                        }
                    }

                    // Check for number
                    if chars[i].is_ascii_digit() || (chars[i] == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
                        let start_pos = i;
                        
                        // Handle hex/binary/octal
                        if chars[i] == '0' && i + 1 < chars.len() {
                            match chars[i + 1] {
                                'x' | 'X' => {
                                    i += 2;
                                    while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                                        i += 1;
                                    }
                                }
                                'b' | 'B' => {
                                    i += 2;
                                    while i < chars.len() && (chars[i] == '0' || chars[i] == '1' || chars[i] == '_') {
                                        i += 1;
                                    }
                                }
                                'o' | 'O' => {
                                    i += 2;
                                    while i < chars.len() && (('0'..='7').contains(&chars[i]) || chars[i] == '_') {
                                        i += 1;
                                    }
                                }
                                _ => {
                                    // Decimal or float
                                    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_' || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E') {
                                        if (chars[i] == 'e' || chars[i] == 'E') && i + 1 < chars.len() && (chars[i + 1] == '+' || chars[i + 1] == '-') {
                                            i += 1;
                                        }
                                        i += 1;
                                    }
                                }
                            }
                        } else {
                            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_' || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E') {
                                if (chars[i] == 'e' || chars[i] == 'E') && i + 1 < chars.len() && (chars[i + 1] == '+' || chars[i + 1] == '-') {
                                    i += 1;
                                }
                                i += 1;
                            }
                        }
                        
                        // Type suffix (e.g., u32, f64)
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            i += 1;
                        }
                        
                        tokens.push(Token::new(TokenType::Number, start_pos, i));
                        continue;
                    }

                    // Check for identifier/keyword
                    if chars[i].is_alphabetic() || chars[i] == '_' {
                        let start_pos = i;
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            i += 1;
                        }
                        
                        let word: String = chars[start_pos..i].iter().collect();
                        
                        let token_type = if self.language.keywords.contains(&word) {
                            TokenType::Keyword
                        } else if self.language.types.contains(&word) {
                            TokenType::Type
                        } else if self.language.builtins.contains(&word) {
                            TokenType::Function
                        } else if word.chars().all(|c| c.is_uppercase() || c == '_') {
                            TokenType::Constant
                        } else if i < chars.len() && chars[i] == '(' {
                            TokenType::Function
                        } else if i < chars.len() && chars[i] == '!' {
                            TokenType::Macro
                        } else {
                            TokenType::Variable
                        };
                        
                        tokens.push(Token::new(token_type, start_pos, i));
                        continue;
                    }

                    // Check for preprocessor
                    if chars[i] == '#' {
                        let start_pos = i;
                        while i < chars.len() && !chars[i].is_whitespace() {
                            i += 1;
                        }
                        tokens.push(Token::new(TokenType::Preprocessor, start_pos, i));
                        continue;
                    }

                    // Check for attribute (Rust)
                    if chars[i] == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
                        let start_pos = i;
                        let mut depth = 0;
                        while i < chars.len() {
                            if chars[i] == '[' {
                                depth += 1;
                            } else if chars[i] == ']' {
                                depth -= 1;
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            i += 1;
                        }
                        tokens.push(Token::new(TokenType::Attribute, start_pos, i));
                        continue;
                    }

                    // Operators and punctuation
                    let start_pos = i;
                    let c = chars[i];
                    i += 1;
                    
                    let token_type = match c {
                        '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '!' | '&' | '|' | '^' | '~' => TokenType::Operator,
                        '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':' | '.' | '?' | '@' => TokenType::Punctuation,
                        _ => TokenType::Text,
                    };
                    
                    // Handle multi-char operators
                    if i < chars.len() {
                        let next = chars[i];
                        let is_double_op = matches!((c, next),
                            ('=', '=') | ('!', '=') | ('<', '=') | ('>', '=') |
                            ('&', '&') | ('|', '|') | ('+', '+') | ('-', '-') |
                            ('+', '=') | ('-', '=') | ('*', '=') | ('/', '=') |
                            ('<', '<') | ('>', '>') | ('-', '>') | ('=', '>') |
                            (':', ':')
                        );
                        if is_double_op {
                            i += 1;
                        }
                    }
                    
                    tokens.push(Token::new(token_type, start_pos, i));
                }
                LexerState::InBlockComment => {
                    let start_pos = i;
                    if let Some((_, ref end)) = self.language.block_comment {
                        if let Some(pos) = line[i..].find(end.as_str()) {
                            tokens.push(Token::new(TokenType::Comment, start_pos, i + pos + end.len()));
                            i += pos + end.len();
                            self.state = LexerState::Normal;
                            continue;
                        }
                    }
                    tokens.push(Token::new(TokenType::Comment, start_pos, chars.len()));
                    return tokens;
                }
                LexerState::InString(delim) => {
                    let start_pos = i;
                    while i < chars.len() {
                        if chars[i] == '\\' && i + 1 < chars.len() {
                            i += 2;
                        } else if chars[i] == delim {
                            i += 1;
                            self.state = LexerState::Normal;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    tokens.push(Token::new(TokenType::String, start_pos, i));
                }
                LexerState::InRawString(_) => {
                    // Handle Rust raw strings r#"..."#
                    tokens.push(Token::new(TokenType::String, 0, chars.len()));
                    return tokens;
                }
            }
        }

        tokens
    }
}

// =============================================================================
// Language Registry
// =============================================================================

/// Registry of supported languages.
pub struct LanguageRegistry {
    languages: Vec<Language>,
}

impl LanguageRegistry {
    /// Creates a new registry with built-in languages.
    pub fn new() -> Self {
        Self {
            languages: vec![
                Language::rust(),
                Language::javascript(),
                Language::python(),
                Language::c(),
            ],
        }
    }

    /// Finds a language by file extension.
    pub fn find_by_extension(&self, ext: &str) -> Option<&Language> {
        self.languages.iter().find(|lang| lang.extensions.iter().any(|e| e == ext))
    }

    /// Finds a language by name.
    pub fn find_by_name(&self, name: &str) -> Option<&Language> {
        self.languages.iter().find(|lang| lang.name.eq_ignore_ascii_case(name))
    }

    /// Lists all language names.
    pub fn list(&self) -> Vec<&str> {
        self.languages.iter().map(|l| l.name.as_str()).collect()
    }

    /// Adds a language.
    pub fn add(&mut self, language: Language) {
        self.languages.push(language);
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_keywords() {
        let lang = Language::rust();
        let mut hl = Highlighter::new(lang);
        let tokens = hl.highlight_line("fn main() {");
        
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Keyword)); // fn
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Function)); // main
    }

    #[test]
    fn test_string_highlight() {
        let lang = Language::rust();
        let mut hl = Highlighter::new(lang);
        let tokens = hl.highlight_line("let s = \"hello\";");
        
        assert!(tokens.iter().any(|t| t.token_type == TokenType::String));
    }

    #[test]
    fn test_comment_highlight() {
        let lang = Language::rust();
        let mut hl = Highlighter::new(lang);
        let tokens = hl.highlight_line("// This is a comment");
        
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].token_type, TokenType::Comment);
    }

    #[test]
    fn test_number_highlight() {
        let lang = Language::rust();
        let mut hl = Highlighter::new(lang);
        let tokens = hl.highlight_line("let x = 0x1234_abcd;");
        
        assert!(tokens.iter().any(|t| t.token_type == TokenType::Number));
    }
}
