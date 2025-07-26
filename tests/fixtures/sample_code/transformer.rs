//! A sample code transformer module for testing embeddings

use std::fmt;

pub trait Transform {
    fn transform(&self, input: &str) -> String;
    fn inverse(&self, input: &str) -> String;
}

pub struct CaseTransformer {
    to_uppercase: bool,
}

impl CaseTransformer {
    pub fn new(to_uppercase: bool) -> Self {
        Self { to_uppercase }
    }
    
    pub fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        let mut prev_is_upper = false;
        
        for (i, ch) in s.chars().enumerate() {
            if ch.is_uppercase() && i > 0 && !prev_is_upper {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
            prev_is_upper = ch.is_uppercase();
        }
        
        result
    }
    
    pub fn to_camel_case(s: &str) -> String {
        let mut result = String::new();
        let mut capitalize_next = false;
        
        for (i, ch) in s.chars().enumerate() {
            if ch == '_' {
                capitalize_next = true;
            } else if capitalize_next || i == 0 {
                result.push(ch.to_uppercase().next().unwrap());
                capitalize_next = false;
            } else {
                result.push(ch);
            }
        }
        
        result
    }
}

impl Transform for CaseTransformer {
    fn transform(&self, input: &str) -> String {
        if self.to_uppercase {
            input.to_uppercase()
        } else {
            input.to_lowercase()
        }
    }
    
    fn inverse(&self, input: &str) -> String {
        if self.to_uppercase {
            input.to_lowercase()
        } else {
            input.to_uppercase()
        }
    }
}

pub struct StringTransformer;

impl StringTransformer {
    pub fn reverse(s: &str) -> String {
        s.chars().rev().collect()
    }
    
    pub fn remove_whitespace(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }
    
    pub fn pad_left(s: &str, width: usize, pad_char: char) -> String {
        if s.len() >= width {
            s.to_string()
        } else {
            format!("{}{}", pad_char.to_string().repeat(width - s.len()), s)
        }
    }
    
    pub fn pad_right(s: &str, width: usize, pad_char: char) -> String {
        if s.len() >= width {
            s.to_string()
        } else {
            format!("{}{}", s, pad_char.to_string().repeat(width - s.len()))
        }
    }
}

impl fmt::Display for CaseTransformer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CaseTransformer(to_uppercase={})", self.to_uppercase)
    }
}