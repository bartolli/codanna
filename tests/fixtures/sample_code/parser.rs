//! A sample parser module for testing embeddings

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]
pub struct ParseError {
    message: String,
    line: usize,
    column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Parse error at {}:{}: {}", self.line, self.column, self.message)
    }
}

impl Error for ParseError {}

pub trait Parser {
    type Output;
    
    fn parse(&self, input: &str) -> Result<Self::Output, ParseError>;
    fn validate(&self, input: &str) -> bool;
}

pub struct JsonParser {
    strict_mode: bool,
}

impl JsonParser {
    pub fn new(strict_mode: bool) -> Self {
        Self { strict_mode }
    }
    
    pub fn parse_string(&self, s: &str) -> Result<String, ParseError> {
        // Simple string parsing logic
        if s.starts_with('"') && s.ends_with('"') {
            Ok(s[1..s.len()-1].to_string())
        } else {
            Err(ParseError {
                message: "Invalid string format".to_string(),
                line: 1,
                column: 1,
            })
        }
    }
    
    pub fn parse_number(&self, s: &str) -> Result<f64, ParseError> {
        s.parse::<f64>().map_err(|_| ParseError {
            message: "Invalid number format".to_string(),
            line: 1,
            column: 1,
        })
    }
}

impl Parser for JsonParser {
    type Output = serde_json::Value;
    
    fn parse(&self, input: &str) -> Result<Self::Output, ParseError> {
        serde_json::from_str(input).map_err(|e| ParseError {
            message: e.to_string(),
            line: e.line(),
            column: e.column(),
        })
    }
    
    fn validate(&self, input: &str) -> bool {
        self.parse(input).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_string() {
        let parser = JsonParser::new(true);
        assert_eq!(parser.parse_string("\"hello\"").unwrap(), "hello");
        assert!(parser.parse_string("hello").is_err());
    }
    
    #[test]
    fn test_parse_number() {
        let parser = JsonParser::new(true);
        assert_eq!(parser.parse_number("42").unwrap(), 42.0);
        assert_eq!(parser.parse_number("3.14").unwrap(), 3.14);
        assert!(parser.parse_number("abc").is_err());
    }
}