use regex::Regex;

use super::output::RewriteOutput;

pub struct KiriKiriOutput {
    pub replace_rule: Regex,
    pub capture_rule: Regex,
}

impl KiriKiriOutput {
    pub fn new(replace_rule: &str, capture_rule: &str) -> Self {
        let replace_rule = Regex::new(&replace_rule).unwrap();
        let capture_rule = Regex::new(&capture_rule).unwrap();
        Self {
            replace_rule,
            capture_rule,
        }
    }
}

impl RewriteOutput for KiriKiriOutput {
    fn extract_lines(&self, content: &str) -> Vec<String> {
        let mut lines = vec![];
        let content = self.replace_rule.replace_all(content, "\\n").to_string();
        self.capture_rule.captures_iter(&content).for_each(|cap| {
            lines.push(cap[1].to_string().replace("\"", ""));
        });
        lines
    }
    fn format_line(&self, _: &str, translated_line: &str) -> String {
        format!("{}\n", translated_line)
    }
}
