use super::{output::RewriteOutput, text::TextOutput};

pub struct MToolOutput {
    text_output: TextOutput,
}

impl MToolOutput {
    pub fn new(replace_rule: &str, capture_rule: &str) -> Self {
        Self {
            text_output: TextOutput::new(replace_rule, capture_rule),
        }
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
            escape_json_string(content)
        )
    }
}

fn escape_json_string(s: &str) -> String {
    let mut escaped = String::new();
    for c in s.chars() {
        match c {
            '"' => escaped.push_str(r#"\""#),
            '\\' => escaped.push_str(r#"\\"#),
            '\x08' => escaped.push_str(r#"\b"#),
            '\x0c' => escaped.push_str(r#"\f"#),
            '\n' => escaped.push_str(r#"\n"#),
            '\r' => escaped.push_str(r#"\r"#),
            '\t' => escaped.push_str(r#"\t"#),
            _ => escaped.push(c),
        }
    }
    escaped
}
