use tree_sitter::{Node, Parser};

use crate::{
    Language,
    frontend::{FrontendError, shared::*},
    ir::{Expr, Import, ImportBinding, Module, Stmt},
};

const LANG: Language = Language::JavaScript;

pub fn parse(module_id: &str, source: &str) -> Result<Module, FrontendError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|_| FrontendError::Parser(LANG))?;
    let tree = parser
        .parse(source, None)
        .ok_or(FrontendError::Parser(LANG))?;
    if tree.root_node().has_error() {
        let p = tree.root_node().start_position();
        return Err(FrontendError::Syntax {
            language: LANG,
            module_id: module_id.into(),
            line: p.row + 1,
            column: p.column + 1,
        });
    }
    let mut module = Module {
        id: module_id.into(),
        source_language: LANG,
        imports: vec![],
        exports: vec![],
        body: vec![],
    };
    for node in named_children(tree.root_node()) {
        match node.kind() {
            "import_statement" => module.imports.push(parse_import(node, source)),
            "export_statement" => {
                if let Some(decl) = node
                    .child_by_field_name("declaration")
                    .or_else(|| node.named_child(0))
                {
                    module.body.push(stmt(decl, source, module_id)?);
                }
            }
            _ => module.body.extend(stmt_many(node, source, module_id)?),
        }
    }
    module.exports = inferred_exports(&module.body);
    Ok(module)
}

fn parse_import(node: Node<'_>, source: &str) -> Import {
    let source_node = node.child_by_field_name("source").or_else(|| {
        named_children(node)
            .into_iter()
            .find(|n| n.kind() == "string")
    });
    let spec = source_node
        .map(|n| unquote(text(n, source)))
        .unwrap_or_default();
    let mut bindings = vec![];
    for child in named_children(node) {
        parse_import_bindings(child, source, &mut bindings);
    }
    Import {
        source: spec,
        bindings,
        external: false,
    }
}

fn parse_import_bindings(node: Node<'_>, source: &str, bindings: &mut Vec<ImportBinding>) {
    match node.kind() {
        "identifier" => bindings.push(ImportBinding {
            imported: "default".into(),
            local: text(node, source).into(),
        }),
        "namespace_import" => {
            if let Some(id) = named_children(node)
                .into_iter()
                .find(|n| n.kind() == "identifier")
            {
                bindings.push(ImportBinding {
                    imported: "*".into(),
                    local: text(id, source).into(),
                });
            }
        }
        "import_specifier" => {
            if let Some(name) = node.child_by_field_name("name") {
                let alias = node.child_by_field_name("alias").unwrap_or(name);
                bindings.push(ImportBinding {
                    imported: unquote(text(name, source)),
                    local: text(alias, source).into(),
                });
            }
        }
        "import_clause" | "named_imports" => {
            for child in named_children(node) {
                parse_import_bindings(child, source, bindings);
            }
        }
        _ => {}
    }
}

fn stmt_many(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
        let mutable = text(node, source).trim_start().starts_with("let")
            || text(node, source).trim_start().starts_with("var");
        return named_children(node)
            .into_iter()
            .filter(|n| n.kind() == "variable_declarator")
            .map(|decl| {
                let name = decl
                    .child_by_field_name("name")
                    .ok_or_else(|| unsupported(LANG, module_id, decl))?;
                if name.kind() != "identifier" {
                    return Err(unsupported(LANG, module_id, name));
                }
                let value = decl
                    .child_by_field_name("value")
                    .map(|n| expr(n, source, module_id))
                    .transpose()?;
                Ok(Stmt::Let {
                    name: text(name, source).into(),
                    value,
                    mutable,
                })
            })
            .collect();
    }
    Ok(vec![stmt(node, source, module_id)?])
}

fn stmt(node: Node<'_>, source: &str, module_id: &str) -> Result<Stmt, FrontendError> {
    match node.kind() {
        "function_declaration" => {
            let name = required(node, "name", module_id)?;
            let params = named_children(required(node, "parameters", module_id)?)
                .into_iter()
                .filter(|n| n.kind() == "identifier")
                .map(|n| text(n, source).into())
                .collect();
            Ok(Stmt::Function {
                name: text(name, source).into(),
                params,
                body: block(required(node, "body", module_id)?, source, module_id)?,
            })
        }
        "expression_statement" => {
            let value = node
                .named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            if value.kind() == "assignment_expression" {
                Ok(Stmt::Assign {
                    target: expr(required(value, "left", module_id)?, source, module_id)?,
                    value: expr(required(value, "right", module_id)?, source, module_id)?,
                })
            } else {
                Ok(Stmt::Expr(expr(value, source, module_id)?))
            }
        }
        "return_statement" => Ok(Stmt::Return {
            value: node
                .named_child(0)
                .map(|n| expr(n, source, module_id))
                .transpose()?,
        }),
        "if_statement" => Ok(Stmt::If {
            condition: expr(required(node, "condition", module_id)?, source, module_id)?,
            then_body: as_body(required(node, "consequence", module_id)?, source, module_id)?,
            else_body: node
                .child_by_field_name("alternative")
                .map(|n| as_body(n, source, module_id))
                .transpose()?
                .unwrap_or_default(),
        }),
        "while_statement" => Ok(Stmt::While {
            condition: expr(required(node, "condition", module_id)?, source, module_id)?,
            body: as_body(required(node, "body", module_id)?, source, module_id)?,
        }),
        "empty_statement" => Ok(Stmt::Expr(Expr::Null)),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn block(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    let mut out = vec![];
    for child in named_children(node) {
        out.extend(stmt_many(child, source, module_id)?);
    }
    Ok(out)
}

fn as_body(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    if node.kind() == "statement_block" {
        block(node, source, module_id)
    } else {
        stmt_many(node, source, module_id)
    }
}

fn expr(node: Node<'_>, source: &str, module_id: &str) -> Result<Expr, FrontendError> {
    match node.kind() {
        "identifier" | "this" => Ok(Expr::Name(text(node, source).into())),
        "number" => Ok(Expr::Number(text(node, source).replace('_', ""))),
        "string" | "template_string" => Ok(Expr::String(unquote(text(node, source)))),
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        "null" | "undefined" => Ok(Expr::Null),
        "array" => Ok(Expr::Array(
            named_children(node)
                .into_iter()
                .map(|n| expr(n, source, module_id))
                .collect::<Result<_, _>>()?,
        )),
        "object" => {
            let mut values = vec![];
            for pair in named_children(node) {
                if pair.kind() != "pair" {
                    return Err(unsupported(LANG, module_id, pair));
                }
                let key = required(pair, "key", module_id)?;
                let key_expr = if key.kind() == "property_identifier" {
                    Expr::String(text(key, source).into())
                } else {
                    expr(key, source, module_id)?
                };
                values.push((
                    key_expr,
                    expr(required(pair, "value", module_id)?, source, module_id)?,
                ));
            }
            Ok(Expr::Object(values))
        }
        "binary_expression" => binary(node, source, module_id),
        "unary_expression" => {
            let operand = required(node, "argument", module_id)?;
            let op = unary_op(source[node.start_byte()..operand.start_byte()].trim())
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            Ok(Expr::Unary {
                op,
                operand: Box::new(expr(operand, source, module_id)?),
            })
        }
        "call_expression" => {
            let args = required(node, "arguments", module_id)?;
            Ok(Expr::Call {
                callee: Box::new(expr(
                    required(node, "function", module_id)?,
                    source,
                    module_id,
                )?),
                args: named_children(args)
                    .into_iter()
                    .map(|n| expr(n, source, module_id))
                    .collect::<Result<_, _>>()?,
            })
        }
        "member_expression" | "subscript_expression" => {
            let object = required(node, "object", module_id)?;
            let property = required(
                node,
                if node.kind() == "member_expression" {
                    "property"
                } else {
                    "index"
                },
                module_id,
            )?;
            let computed = node.kind() == "subscript_expression";
            if !computed && text(object, source) == "console" && text(property, source) == "log" {
                return Ok(Expr::Intrinsic(crate::ir::Intrinsic::Print));
            }
            let property = if !computed && property.kind() == "property_identifier" {
                Expr::Name(text(property, source).into())
            } else {
                expr(property, source, module_id)?
            };
            Ok(Expr::Member {
                object: Box::new(expr(object, source, module_id)?),
                property: Box::new(property),
                computed,
            })
        }
        "parenthesized_expression" => expr(
            node.named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?,
            source,
            module_id,
        ),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn binary(node: Node<'_>, source: &str, module_id: &str) -> Result<Expr, FrontendError> {
    let left = required(node, "left", module_id)?;
    let right = required(node, "right", module_id)?;
    let op = binary_op(source[left.end_byte()..right.start_byte()].trim())
        .ok_or_else(|| unsupported(LANG, module_id, node))?;
    Ok(Expr::Binary {
        op,
        left: Box::new(expr(left, source, module_id)?),
        right: Box::new(expr(right, source, module_id)?),
    })
}

fn required<'a>(node: Node<'a>, field: &str, module_id: &str) -> Result<Node<'a>, FrontendError> {
    node.child_by_field_name(field)
        .ok_or_else(|| unsupported(LANG, module_id, node))
}
