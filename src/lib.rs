use std::fs;

use anyhow::Result;
use clap::Parser;
use input::input;
use input::TransType;
use isolang::Language;
use output::output;
use serde::{Deserialize, Serialize};
use translator::{translate, ChatGPTOptions};

mod input;
mod output;
mod textures;
mod translator;
mod utils;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexDescription {
    pub usage: RegexUsage,
    pub regex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegexUsage {
    #[serde(rename = "replace")]
    Replace(String),
    #[serde(rename = "capture")]
    Capture(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub file: Option<String>,
    /// iso 639-3 code, see https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
    #[serde(rename = "from")]
    pub lang_from: Language,
    /// iso 639-3 code, see https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
    #[serde(rename = "to")]
    pub lang_to: Language,
    pub trans_type: TransType,
    /// filter the input lines by regex, only the lines that match the regex will be translated, if
    /// empty, all lines will be translated
    pub filter_regexen: Vec<String>,
    /// capture the text by regex, and replace the text by replace_expression;
    pub capture_regex: Option<String>,
    /// replace the text by replace_expression, must contain flag $trans, $trans will be replaced
    /// by the translated text, example: [: "$trans"];
    pub replace_expression: Option<String>,
    pub output_regexen: Vec<RegexDescription>,
    pub chatgpt_opt: Option<ChatGPTOptions>,
    pub specify_range: Option<Vec<(usize, usize)>>,
    pub batchizer_opt: BatchizerOptions,
    pub mtool_opt: Option<MToolOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MToolOptions {
    pub line_width: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchizerOptions {
    pub max_tokens: usize,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Arguments {
    /// Input file, It's Optional, override the file in [config|default].toml;
    #[arg(global = true)]
    pub file: Option<String>,
    /// Configuration file, It's Required;
    #[arg(short, long, default_value = "default.toml")]
    pub config: String,
    /// just output the result from file.textures.json, without translate;
    #[arg(short = 'j', long = "outputonly", default_value_t = false)]
    pub output_only: bool,
}

pub async fn start(args: Arguments) -> Result<()> {
    let mut cfg = { toml::from_str::<Configuration>(&fs::read_to_string(args.config)?)? };

    let file = match args.file {
        Some(v) => v,
        None => match &cfg.file {
            Some(v) => v.clone(),
            None => {
                return Err(anyhow::anyhow!("No input file specified"));
            }
        },
    };

    cfg.specify_range = {
        match fs::OpenOptions::new()
            .read(true)
            .open(format!("{}.dignostic_failed_range.json", file))
        {
            Ok(v) => match serde_json::from_reader::<_, Vec<(usize, usize)>>(v) {
                Ok(v) => {
                    println!("load specify range");
                    Some(v)
                }
                _ => None,
            },
            _ => None,
        }
    };
    // input
    let textures = input(cfg.trans_type, &file, cfg.filter_regexen.clone())?;

    if args.output_only {
        return output(&cfg, &textures);
    }

    let mut textures_mut = textures.clone();
    translate(textures, &mut textures_mut, &cfg).await?;
    output(&cfg, &textures_mut)
}

pub struct Timer {
    start: std::time::Instant,
    interval: std::time::Duration,
}

impl Timer {
    pub fn new(interval: std::time::Duration) -> Self {
        Self {
            start: std::time::Instant::now(),
            interval,
        }
    }
    pub fn finished(&mut self) -> bool {
        if self.start.elapsed() >= self.interval {
            self.reset();
            true
        } else {
            false
        }
    }
    fn reset(&mut self) {
        self.start = std::time::Instant::now();
    }
}

#[cfg(test)]
mod test {
    use crate::{Configuration, MToolOptions};

    #[test]
    fn options_deserialize() {
        let str = include_str!("../assets/options_mtool.toml");
        let config: Configuration = toml::from_str(str).unwrap();
        config.output_regexen.iter().for_each(|x| {
            println!("{:?}", x);
        });
        assert_eq!(
            config.mtool_opt,
            Some(MToolOptions {
                line_width: Some(36)
            })
        );
        assert_eq!(config.lang_to.to_name(), "Chinese");
    }
}
