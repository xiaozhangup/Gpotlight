use crate::plugin::{PluginAction, SearchPlugin, SearchResult};

pub(super) struct CalculatorPlugin;

impl SearchPlugin for CalculatorPlugin {
    fn id(&self) -> &str {
        "builtin.calculator"
    }

    fn name(&self) -> &str {
        "Calculator"
    }

    fn description(&self) -> &str {
        "Evaluate simple arithmetic expressions"
    }

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let expr = query.trim();
        if expr.is_empty() || !expr.chars().any(|c| "+-*/".contains(c)) {
            return Vec::new();
        }

        match eval_arithmetic(expr) {
            Some(value) => vec![SearchResult {
                title: format_number(value),
                subtitle: expr.to_string(),
                icon: Some("accessories-calculator-symbolic".to_string()),
                pinned: true,
                action: PluginAction::CopyText(format_number(value)),
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            }],
            None => Vec::new(),
        }
    }
}

fn eval_arithmetic(input: &str) -> Option<f64> {
    let mut parser = ArithmeticParser::new(input);
    let value = parser.parse_expr()?;
    parser.skip_ws();
    parser.is_done().then_some(value)
}

fn format_number(value: f64) -> String {
    let text = format!("{value:.12}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct ArithmeticParser<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> ArithmeticParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            cursor: 0,
        }
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') => {
                    self.cursor += 1;
                    value += self.parse_term()?;
                }
                Some(b'-') => {
                    self.cursor += 1;
                    value -= self.parse_term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') => {
                    self.cursor += 1;
                    value *= self.parse_factor()?;
                }
                Some(b'/') => {
                    self.cursor += 1;
                    value /= self.parse_factor()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_ws();
        if self.peek() == Some(b'(') {
            self.cursor += 1;
            let value = self.parse_expr()?;
            self.skip_ws();
            (self.peek()? == b')').then(|| self.cursor += 1)?;
            return Some(value);
        }

        let start = self.cursor;
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.')) {
            self.cursor += 1;
        }
        std::str::from_utf8(&self.input[start..self.cursor])
            .ok()?
            .parse()
            .ok()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn is_done(&self) -> bool {
        self.cursor == self.input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_parser_handles_basic_expressions() {
        assert_eq!(eval_arithmetic("1+3"), Some(4.0));
        assert_eq!(eval_arithmetic("2 * (3 + 4)"), Some(14.0));
        assert_eq!(eval_arithmetic("10 / 2 - 1"), Some(4.0));
    }

    #[test]
    fn calculator_plugin_copies_result_text() {
        let results = CalculatorPlugin.query("1+3");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "4");
        assert!(results[0].pinned);
        assert!(matches!(results[0].action, PluginAction::CopyText(ref text) if text == "4"));
    }
}
