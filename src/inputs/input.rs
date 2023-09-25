use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;

use crate::textures::TextureLine;
use crate::textures::Textures;
use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;

pub fn input(trans_type: TransType, file: &str, regexen: Vec<String>) -> Result<Textures> {
    let textures = match trans_type {
        TransType::Text | TransType::Replace => TextInput::new(regexen).read(file)?,
    };
    Ok(textures)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransType {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "replace")]
    Replace,
}

pub trait Input {
    fn read(&self, file_path: &str) -> Result<Textures> {
        match Textures::load(file_path) {
            Ok(textures) => {
                println!("Loaded textures from {}.textures.json", file_path);
                Ok(textures)
            }
            Err(_) => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .open(file_path)
                    .unwrap_or_else(|_| panic!("Failed to open file: {}", file_path));
                let mut reader = BufReader::new(file);
                let mut textures = self.parse(&mut reader)?;
                println!(
                    "new textures from {}, lines {}",
                    file_path,
                    textures.lines.len()
                );
                textures.name.push_str(file_path);
                Ok(textures)
            }
        }
    }
    fn parse<R: Read>(&self, reader: &mut BufReader<R>) -> Result<Textures> {
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
        Ok(Textures {
            lines: texture_lines,
            curr_index: 0,
            name: String::new(),
        })
    }
    fn extract_line(&self, line: &str) -> Option<String>;
}

pub struct TextInput {
    pub regexen: Vec<Regex>,
}

impl TextInput {
    pub fn new(regexen: Vec<String>) -> Self {
        let regexen = regexen
            .into_iter()
            .map(|re| Regex::new(&re).unwrap())
            .collect::<Vec<_>>();
        Self { regexen }
    }
}

impl Input for TextInput {
    fn extract_line(&self, line: &str) -> Option<String> {
        if self.regexen.is_empty() {
            if line.trim().is_empty() {
                None
            } else {
                Some(line.to_string())
            }
        } else {
            for regex in &self.regexen {
                if regex.is_match(line) {
                    return Some(line.to_string());
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mtool_input() {
        let content = r#"
{
    "100": "100",
    "BGM": "BGM",
    "请原谅我": "请原谅我",
    "请原\"谅\"我": "请原\"谅\"我",
    "请原\"谅\"我": "请原\"谅\"我"
}
"#;
        let mut reader = BufReader::new(content.as_bytes());
        let re = r#"^\s*".*[^\x00-\x7f].*"#;
        let textures = TextInput::new(vec![re.to_string()])
            .parse(&mut reader)
            .unwrap();
        textures.lines.iter().for_each(|line| {
            println!("{}", line.content);
        });
        assert_eq!(textures.lines.len(), 3);
    }

    #[test]
    fn test_text_input() {
        let content = r#"
        你好。
        I'm fine.
        Good morning.
"#;
        let mut reader = BufReader::new(content.as_bytes());
        let re = r#"^\s*.*[^\x00-\x7f].*"#;
        let textures = TextInput::new(vec![re.to_string()])
            .parse(&mut reader)
            .unwrap();
        assert_eq!(textures.lines.len(), 1);
        let mut reader = BufReader::new(content.as_bytes());
        let textures = TextInput::new(vec![]).parse(&mut reader).unwrap();
        assert_eq!(textures.lines.len(), 3);
    }

    #[test]
    fn test_kiri_kiri_ks_input() {
        let content = r#"
;//======================================================//
;//■ファイル名：00_00
;//======================================================//
*00_01|思春期
[cm]

[bgm file="BGM05"]

;//★ＥＶ：聡志×陽子：側位　表情１／ペニス１／陽子の手０
[ev file="EVA01_01" rule="rule14"]

[cn name="聡　志"]
Hello.
[en]
*save|

[cn name="陽　子" voice="00_01_001"]
你好。
[en]
*save|

[cn name="陽　子" spvoice="00_01_002" storage="ト書き"]
今天天气不错
[en]
*save|
"#;
        let mut reader = BufReader::new(content.as_bytes());
        let re = r#"^[^;*\[\n]\s*[^\s]+"#;
        let textures = TextInput::new(vec![re.to_string()])
            .parse(&mut reader)
            .unwrap();
        assert_eq!(textures.lines.len(), 3);
    }

    #[test]
    fn test_ain_input() {
        let content = r#"
; 注释
;s[2227] = "角色1"
;s[2800] = "10801"
;m[293] = "好的"
;s[1934] = "角色2"
;s[2801] = "10802"
;m[300] = "请原谅我"
;m[300] = "请\"原谅\"我"
;m[300] = ""
"#;
        let mut reader = BufReader::new(content.as_bytes());
        let re = r#"^;m\[\d+\]\s=\s".+""#;
        let textures = TextInput::new(vec![re.to_string()])
            .parse(&mut reader)
            .unwrap();
        assert_eq!(textures.lines.len(), 3);
    }
}
