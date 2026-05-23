use tessera_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub message: String,
    pub primary_span: Span,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(rule_id: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self { rule_id, severity: Severity::Error, message: message.into(), primary_span: span, help: None }
    }

    pub fn warn(rule_id: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self { rule_id, severity: Severity::Warn, message: message.into(), primary_span: span, help: None }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}
