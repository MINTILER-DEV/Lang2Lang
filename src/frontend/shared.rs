use tree_sitter::Node;

use crate::{
    Language,
    frontend::FrontendError,
    ir::{BinaryOp, Stmt, UnaryOp},
};

pub fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

pub fn inferred_exports(body: &[Stmt]) -> Vec<String> {
    body.iter()
        .filter_map(|statement| match statement {
            Stmt::Function { name, .. } | Stmt::Let { name, .. } if !name.starts_with('_') => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect()
}

pub fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn unsupported(language: Language, module_id: &str, node: Node<'_>) -> FrontendError {
    let point = node.start_position();
    FrontendError::Unsupported {
        language,
        module_id: module_id.to_owned(),
        syntax: node.kind().to_owned(),
        line: point.row + 1,
        column: point.column + 1,
    }
}

pub fn binary_op(operator: &str) -> Option<BinaryOp> {
    Some(match operator {
        "+" => BinaryOp::Add,
        "-" => BinaryOp::Subtract,
        "*" => BinaryOp::Multiply,
        "/" => BinaryOp::Divide,
        "//" => BinaryOp::FloorDivide,
        "%" => BinaryOp::Modulo,
        "**" | "^" => BinaryOp::Power,
        "==" | "===" => BinaryOp::Equal,
        "!=" | "!==" | "~=" => BinaryOp::NotEqual,
        "<" => BinaryOp::Less,
        "<=" => BinaryOp::LessEqual,
        ">" => BinaryOp::Greater,
        ">=" => BinaryOp::GreaterEqual,
        "and" | "&&" => BinaryOp::And,
        "or" | "||" => BinaryOp::Or,
        _ => return None,
    })
}

pub fn unary_op(operator: &str) -> Option<UnaryOp> {
    Some(match operator {
        "not" | "!" => UnaryOp::Not,
        "-" => UnaryOp::Negate,
        "+" => UnaryOp::Positive,
        _ => return None,
    })
}

pub fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0] as char;
        let last = value.as_bytes()[value.len() - 1] as char;
        if matches!(first, '\'' | '"' | '`') && first == last {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}
