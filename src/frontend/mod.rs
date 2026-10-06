mod javascript;
mod lua;
mod python;
mod shared;

use thiserror::Error;

use crate::{Language, ir::Module};

#[derive(Debug, Error)]
pub enum FrontendError {
    #[error("{language} syntax error in {module_id} near line {line}, column {column}")]
    Syntax {
        language: Language,
        module_id: String,
        line: usize,
        column: usize,
    },
    #[error(
        "unsupported {language} syntax `{syntax}` in {module_id} at line {line}, column {column}"
    )]
    Unsupported {
        language: Language,
        module_id: String,
        syntax: String,
        line: usize,
        column: usize,
    },
    #[error("could not initialize the {0} parser")]
    Parser(Language),
}

pub fn parse(language: Language, module_id: &str, source: &str) -> Result<Module, FrontendError> {
    match language {
        Language::Python => python::parse(module_id, source),
        Language::JavaScript => javascript::parse(module_id, source),
        Language::Lua => lua::parse(module_id, source),
    }
}
