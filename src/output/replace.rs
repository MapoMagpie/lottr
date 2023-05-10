use regex::Regex;

use super::{output::RewriteOutput, text::TextOutput};

pub struct ReplaceOutput {
    text_output: TextOutput,
    line_width: Option<usize>,
    replace_expression: String,
    capture_regex: Regex,
}

impl ReplaceOutput {
    pub fn new(
        replace_rule: &str,
        capture_rule: &str,
        replace_expression: &str,
        capture_regex: &str,
    ) -> Self {
        Self {
            text_output: TextOutput::new(replace_rule, capture_rule),
            line_width: None,
            replace_expression: replace_expression.to_string(),
            capture_regex: Regex::new(capture_regex).unwrap(),
        }
    }

    pub fn set_line_width(&mut self, line_width: Option<usize>) {
        self.line_width = line_width;
    }
}

impl RewriteOutput for ReplaceOutput {
    fn extract_lines(&self, content: &str) -> Vec<String> {
        self.text_output.extract_lines(content)
    }
    fn format_line(&self, raw: &str, content: &str) -> String {
        let content = escape_json_string(content, self.line_width);
        let content = self.replace_expression.replace("$trans", &content);
        let content = self.capture_regex.replace(&raw, content);
        content.to_string()
    }
}

fn escape_json_string(s: &str, line_width: Option<usize>) -> String {
    let line_width = line_width.unwrap_or(3000);
    let mut escaped = String::new();
    let mut line_len = 0;
    for c in s.chars() {
        line_len += 1;
        match c {
            '"' => escaped.push_str(r#"\""#),
            '\\' => escaped.push_str(r#"\\"#),
            '\x08' => escaped.push_str(r#"\b"#),
            '\x0c' => escaped.push_str(r#"\f"#),
            '\n' => {
                line_len = 0;
                escaped.push_str(r#"\n"#);
            }
            '\r' => {
                line_len = 0;
                escaped.push_str(r#"\r"#);
            }
            '\t' => escaped.push_str(r#"\t"#),
            _ => escaped.push(c),
        }
        if line_len >= line_width {
            line_len = 0;
            escaped.push_str(r#"\n"#);
        }
    }
    escaped
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn escape_json_string_test() {
        let s = r#"hello\world"#;
        let escaped = escape_json_string(s, Some(5));
        assert_eq!(escaped, "hello\\n\\\\worl\\nd");
    }

    #[test]
    fn test_format_line_for_mtool() {
        let output = ReplaceOutput::new(r#""(.*)""#, r#""(.*)""#, r#": "$trans""#, r#":\s"(.+)""#);
        let line = output.format_line(r#""请翻译": "待翻译","#, "翻译完成");
        assert_eq!(line, r#""请翻译": "翻译完成","#);
        let content = r#" "请原\"谅\"我": "请原\"谅\"我", "#;
        let line = output.format_line(content, "翻译完成");
        assert_eq!(line, r#" "请原\"谅\"我": "翻译完成", "#);
    }

    #[test]
    fn test_format_line_for_ain() {
        let output = ReplaceOutput::new(r#""(.*)""#, r#""(.*)""#, r#"= "$trans""#, r#"=\s"(.+)""#);
        let content = r#";m[300] = "请原谅我""#;
        let line = output.format_line(content, "翻译完成");
        assert_eq!(line, r#";m[300] = "翻译完成""#);
    }
}
