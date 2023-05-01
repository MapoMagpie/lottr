use std::io::BufRead;
use std::io::BufReader;

use crate::textures::TextureLine;
use crate::textures::Textures;
use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;

pub fn input(trans_type: TransType, file: &str) -> Result<Textures> {
    let textures = match trans_type {
        TransType::Text => TextInput.parser(&file)?,
        TransType::MTool => MToolInput::without_ascii().parser(&file)?,
    };
    Ok(textures)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransType {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "mtool")]
    MTool,
}

pub trait Input {
    fn parser(&self, file_path: &str) -> Result<Textures> {
        match Textures::load(file_path) {
            Ok(textures) => {
                println!("Loaded textures from {}.textures.json", file_path);
                Ok(textures)
            }
            Err(_) => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .open(file_path)
                    .expect(format!("Failed to open file: {}", file_path).as_str());

                let mut reader = BufReader::new(file);
                let mut texture_lines = Vec::new();
                let mut buf = String::new();
                let mut seek = 0;
                loop {
                    let line = reader.read_line(&mut buf);
                    match line {
                        Ok(0) => {
                            break;
                        }
                        Ok(size) => {
                            if let Some(value) = self.extract_line(&buf) {
                                let texture_line = TextureLine::new(seek, size, value, false);
                                texture_lines.push(texture_line);
                            }
                            seek += size;
                            buf.clear();
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                println!(
                    "new textures from {}, lines {}",
                    file_path,
                    texture_lines.len()
                );
                Ok(Textures {
                    lines: texture_lines,
                    curr_index: 0,
                    name: file_path.to_string(),
                })
            }
        }
    }
    fn extract_line(&self, line: &str) -> Option<String>;
}

pub struct TextInput;

impl Input for TextInput {
    fn extract_line(&self, line: &str) -> Option<String> {
        let line = line.trim().to_string();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
}

pub struct MToolInput {
    filter_regex: Option<Regex>,
}

impl MToolInput {
    pub fn new(filter_regex: Option<&str>) -> Self {
        let filter_regex = match filter_regex {
            Some(regex) => Some(Regex::new(regex).unwrap()),
            None => None,
        };
        Self { filter_regex }
    }
    pub fn without_ascii() -> Self {
        Self::new(Some(r"^[\x00-\x7f]+?:"))
    }
    #[allow(unused)]
    pub fn without_filter() -> Self {
        Self::new(None)
    }
}

impl Input for MToolInput {
    fn extract_line(&self, line: &str) -> Option<String> {
        if let Some(regex) = &self.filter_regex {
            if regex.is_match(line) {
                return None;
            }
        }
        let splits = line.split(":").collect::<Vec<_>>();
        if splits.len() != 2 {
            None
        } else {
            let value = splits[0].trim().to_string();
            if value.is_empty() {
                None
            } else {
                let value = value[1..value.len() - 1].to_string();
                if value.is_empty() {
                    None
                } else {
                    if value.trim().is_empty() {
                        None
                    } else {
                        Some(value)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input() {
        let textures = TextInput.parser("test.txt").unwrap();
        assert_eq!(textures.lines.len(), 3);
    }

    #[test]
    fn test_mtool_input() {
        let textures = MToolInput::without_ascii().parser("契约纹.json").unwrap();
        textures.save().unwrap();
    }

    #[test]
    fn test_mtool_extract() {
        let input = MToolInput::without_filter();
        let str = "    \"text\": \"你好\",";
        assert_eq!(input.extract_line(str), Some("text".to_string()));
        let str = " \"test\\\"\": \"\",";
        assert_eq!(input.extract_line(str), Some("test\\\"".to_string()));
        let str = "\"= door ? \\\"door\\\"\": \"= door ? \\\"door\\\"\",";
        println!("{}", str);
        assert_eq!(
            input.extract_line(str),
            Some("= door ? \\\"door\\\"".to_string())
        );
        let str = "  \"  \": \"1\",";
        assert_eq!(input.extract_line(str), None);
        let str = "  \"\"  \": \"1\",";
        assert_eq!(input.extract_line(str), Some("\"  ".to_string()));
    }

    #[test]
    fn test_mtool_extract_without_ascii() {
        let input = MToolInput::without_ascii();
        let str = "    \"text\": \"你好\",";
        assert_eq!(input.extract_line(str), None);
        let str = "\"= door ? \\\"door\\\"\": \"= door ? \\\"door\\\"\",";
        assert_eq!(input.extract_line(str), None);
        let str = "\"= door ?是 \\\"door\\\"\": \"= door ? \\\"door\\\"\",";
        assert_eq!(
            input.extract_line(str),
            Some("= door ?是 \\\"door\\\"".to_string())
        );
        let str = "\"123\": \"123\",";
        assert_eq!(input.extract_line(str), None);
        let str = "\"10\": \"10\",\n";
        assert_eq!(input.extract_line(str), None);
    }
}
