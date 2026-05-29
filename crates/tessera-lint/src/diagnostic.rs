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

    pub fn info(rule_id: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self { rule_id, severity: Severity::Info, message: message.into(), primary_span: span, help: None }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {}

// Manual `miette::Diagnostic` impl because severity is chosen at runtime
// (per-lint), which the derive macro cannot express.
impl miette::Diagnostic for Diagnostic {
    fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        Some(Box::new(self.rule_id))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(match self.severity {
            Severity::Error => miette::Severity::Error,
            Severity::Warn => miette::Severity::Warning,
            Severity::Info => miette::Severity::Advice,
        })
    }

    fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> {
        self.help.as_ref().map(|h| Box::new(h.clone()) as Box<dyn std::fmt::Display>)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
        Some(Box::new(std::iter::once(
            miette::LabeledSpan::new_primary_with_span(None, self.primary_span),
        )))
    }
}
