use serde::{Deserialize, Serialize};

use crate::Language;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub id: String,
    pub source_language: Language,
    pub imports: Vec<Import>,
    pub exports: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub source: String,
    pub bindings: Vec<ImportBinding>,
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportBinding {
    pub imported: String,
    pub local: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Stmt {
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Let {
        name: String,
        value: Option<Expr>,
        mutable: bool,
    },
    Assign {
        target: Expr,
        value: Expr,
    },
    Return {
        value: Option<Expr>,
    },
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Expr {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Name(String),
    Intrinsic(Intrinsic),
    Array(Vec<Expr>),
    Object(Vec<(Expr, Expr)>),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Member {
        object: Box<Expr>,
        property: Box<Expr>,
        computed: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intrinsic {
    Print,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    Not,
    Negate,
    Positive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    FloorDivide,
    Modulo,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}
