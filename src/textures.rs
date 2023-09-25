use std::fs;

use serde::{Deserialize, Serialize};

use crate::translators::Translator;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Textures {
    pub lines: Vec<TextureLine>,
    pub curr_index: usize,
    pub name: String,
}

impl Textures {
    pub fn save(&self) -> Result<(), std::io::Error> {
        println!("Saving textures...");
        let output = format!("{}.textures.json", self.name);
        let file = std::fs::File::create(output)?;
        serde_json::to_writer_pretty(&file, &self)?;
        Ok(())
    }
    pub fn load(file_path: &str) -> Result<Self, std::io::Error> {
        let file_path = format!("{}.textures.json", file_path);
        let file = fs::OpenOptions::new().read(true).open(file_path)?;
        let textures: Textures = serde_json::from_reader(file)?;
        Ok(textures)
    }
    pub fn update(&mut self, change: TranslatedLine) {
        self.curr_index = change.batch_range.1;
        if let Some(line) = self.lines[change.batch_range.0]
            .translated
            .iter_mut()
            .find(|t| t.translator == change.translator)
        {
            line.content = change.content;
            line.batch_range = change.batch_range;
        } else {
            self.lines[change.batch_range.0].translated.push(change);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TextureLine {
    pub seek: usize,
    pub size: usize,
    pub content: String,
    pub skip: bool,
    pub translated: Vec<TranslatedLine>,
}

impl TextureLine {
    pub fn new(seek: usize, size: usize, content: String, skip: bool) -> Self {
        Self {
            seek,
            size,
            content,
            skip,
            translated: vec![],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranslatedLine {
    pub translator: Translator,
    pub content: String,
    // (start, end)
    pub batch_range: (usize, usize),
}

impl TranslatedLine {
    pub fn new(translator: Translator, content: String, start: usize, end: usize) -> Self {
        Self {
            translator,
            content,
            batch_range: (start, end),
        }
    }
}
