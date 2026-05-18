use ingert::scena::Scena;
use ingert_syntax::diag::Errors;
use ingert_syntax::diag::Severity;

#[derive(Debug)]
pub struct ParseError {
    pub messages: Vec<String>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for m in &self.messages {
            writeln!(f, "{m}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// Lex and parse a `.ing` source string into a `Scena` AST.
///
/// # Errors
/// Returns a [`ParseError`] aggregating every diagnostic at `Error` severity or
/// higher emitted by `ingert-syntax`.
pub fn parse_ing(source: &str) -> Result<Scena, ParseError> {
    let mut errors = Errors::new();
    let tokens = ingert_syntax::lex::lex(source, &mut errors);
    if errors.max_severity() >= Severity::Fatal {
        return Err(collect_errors(&errors, source));
    }
    let scena = ingert_syntax::parse::parse(&tokens, &mut errors);
    if errors.max_severity() >= Severity::Error {
        return Err(collect_errors(&errors, source));
    }
    Ok(scena)
}

#[must_use]
pub fn print_ing(scena: &Scena) -> String {
    ingert_syntax::print::print(scena)
}

fn collect_errors(errors: &Errors, source: &str) -> ParseError {
    let msgs = errors
        .errors
        .iter()
        .filter(|d| d.severity >= Severity::Error)
        .map(|d| {
            let line = byte_to_line(source, d.main.span.start);
            format!("L{}: {}", line, d.main.desc)
        })
        .collect();
    ParseError { messages: msgs }
}

fn byte_to_line(src: &str, byte: usize) -> usize {
    let clamped = byte.min(src.len());
    src.as_bytes()
        .iter()
        .take(clamped)
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}
