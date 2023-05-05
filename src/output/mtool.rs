use super::{output::RewriteOutput, text::TextOutput};

pub struct MToolOutput {
    text_output: TextOutput,
    line_width: Option<usize>,
}

impl MToolOutput {
    pub fn new(replace_rule: &str, capture_rule: &str) -> Self {
        Self {
            text_output: TextOutput::new(replace_rule, capture_rule),
            line_width: None,
        }
    }

    pub fn set_line_width(&mut self, line_width: Option<usize>) {
        self.line_width = line_width;
    }
}

impl RewriteOutput for MToolOutput {
    fn extract_lines(&self, content: &str) -> Vec<String> {
        self.text_output.extract_lines(content)
    }
    fn format_line(&self, raw: &str, content: &str) -> String {
        // escape content
        format!(
            "\"{}\": \"{}\",\n",
            raw.trim_matches('\n'),
            escape_json_string(content, self.line_width)
        )
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
}
