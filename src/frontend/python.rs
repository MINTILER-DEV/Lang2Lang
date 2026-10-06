use std::collections::HashSet;

use tree_sitter::{Node, Parser};

use crate::{
    Language,
    frontend::{FrontendError, shared::*},
    ir::{Expr, Import, ImportBinding, Module, Stmt},
};

const LANG: Language = Language::Python;

pub fn parse(module_id: &str, source: &str) -> Result<Module, FrontendError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|_| FrontendError::Parser(LANG))?;
    let tree = parser
        .parse(source, None)
        .ok_or(FrontendError::Parser(LANG))?;
    if let Some(node) = first_error(tree.root_node()) {
        let p = node.start_position();
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
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            module.imports.extend(parse_import(node, source));
        } else {
            module.body.push(stmt(node, source, module_id)?);
        }
    }
    normalize_bindings(&mut module.body, &mut HashSet::new());
    module.exports = inferred_exports(&module.body);
    Ok(module)
}

fn normalize_bindings(body: &mut [Stmt], bindings: &mut HashSet<String>) {
    for statement in body {
        match statement {
            Stmt::Function { name, params, body } => {
                bindings.insert(name.clone());
                let mut function_bindings: HashSet<_> = params.iter().cloned().collect();
                normalize_bindings(body, &mut function_bindings);
            }
            Stmt::Assign {
                target: Expr::Name(name),
                value,
            } if bindings.insert(name.clone()) => {
                *statement = Stmt::Let {
                    name: name.clone(),
                    value: Some(value.clone()),
                    mutable: true,
                };
            }
            Stmt::If {
                then_body,
                else_body,
                ..
            } => {
                normalize_bindings(then_body, bindings);
                normalize_bindings(else_body, bindings);
            }
            Stmt::While { body, .. } => normalize_bindings(body, bindings),
            Stmt::Let { name, .. } => {
                bindings.insert(name.clone());
            }
            _ => {}
        }
    }
}

fn first_error(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).find_map(first_error)
}

fn parse_import(node: Node<'_>, source: &str) -> Vec<Import> {
    let raw = text(node, source).trim();
    if let Some(rest) = raw.strip_prefix("from ")
        && let Some((module, names)) = rest.split_once(" import ")
    {
        let bindings = names
            .trim_matches(|c| c == '(' || c == ')')
            .split(',')
            .map(|part| {
                let mut pieces = part.split_whitespace();
                let imported = pieces.next().unwrap_or_default().to_owned();
                let local = if pieces.next() == Some("as") {
                    pieces.next().unwrap_or(&imported).to_owned()
                } else {
                    imported.clone()
                };
                ImportBinding { imported, local }
            })
            .collect();
        return vec![Import {
            source: python_module_path(module.trim()),
            bindings,
            external: false,
        }];
    }
    raw.strip_prefix("import ")
        .unwrap_or(raw)
        .split(',')
        .map(|part| {
            let mut pieces = part.split_whitespace();
            let source_name = pieces.next().unwrap_or_default();
            let local = if pieces.next() == Some("as") {
                pieces.next().unwrap_or(source_name)
            } else {
                source_name.rsplit('.').next().unwrap_or(source_name)
            };
            Import {
                source: source_name.replace('.', "/"),
                bindings: vec![ImportBinding {
                    imported: "*".into(),
                    local: local.into(),
                }],
                external: false,
            }
        })
        .collect()
}

fn python_module_path(module: &str) -> String {
    let parent_levels = module
        .chars()
        .take_while(|character| *character == '.')
        .count();
    let name = module[parent_levels..].replace('.', "/");
    if parent_levels == 0 {
        name
    } else if parent_levels == 1 {
        format!("./{name}")
    } else {
        format!("{}{}", "../".repeat(parent_levels - 1), name)
            .trim_end_matches('/')
            .to_owned()
    }
}

fn stmt(node: Node<'_>, source: &str, module_id: &str) -> Result<Stmt, FrontendError> {
    match node.kind() {
        "function_definition" => {
            let name = required_field(node, "name", module_id)?;
            let params_node = required_node(node, "parameters", module_id)?;
            let params = named_children(params_node)
                .into_iter()
                .filter(|n| n.kind() == "identifier")
                .map(|n| text(n, source).into())
                .collect();
            let body = block(required_node(node, "body", module_id)?, source, module_id)?;
            Ok(Stmt::Function {
                name: text(name, source).into(),
                params,
                body,
            })
        }
        "expression_statement" => {
            let child = node
                .named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            if child.kind() == "assignment" {
                let left = required_node(child, "left", module_id)?;
                let right = required_node(child, "right", module_id)?;
                Ok(Stmt::Assign {
                    target: expr(left, source, module_id)?,
                    value: expr(right, source, module_id)?,
                })
            } else {
                Ok(Stmt::Expr(expr(child, source, module_id)?))
            }
        }
        "return_statement" => Ok(Stmt::Return {
            value: node
                .named_child(0)
                .map(|n| expr(n, source, module_id))
                .transpose()?,
        }),
        "if_statement" => {
            let condition = expr(
                required_node(node, "condition", module_id)?,
                source,
                module_id,
            )?;
            let then_body = block(
                required_node(node, "consequence", module_id)?,
                source,
                module_id,
            )?;
            let else_body = node
                .child_by_field_name("alternative")
                .map(|n| {
                    if n.kind() == "else_clause" {
                        n.named_child(0)
                            .map(|b| block(b, source, module_id))
                            .transpose()
                            .map(|v| v.unwrap_or_default())
                    } else if n.kind() == "elif_clause" {
                        stmt(n, source, module_id).map(|s| vec![s])
                    } else {
                        block(n, source, module_id)
                    }
                })
                .transpose()?
                .unwrap_or_default();
            Ok(Stmt::If {
                condition,
                then_body,
                else_body,
            })
        }
        "elif_clause" => {
            let condition = expr(
                required_node(node, "condition", module_id)?,
                source,
                module_id,
            )?;
            let then_body = block(
                required_node(node, "consequence", module_id)?,
                source,
                module_id,
            )?;
            Ok(Stmt::If {
                condition,
                then_body,
                else_body: vec![],
            })
        }
        "while_statement" => Ok(Stmt::While {
            condition: expr(
                required_node(node, "condition", module_id)?,
                source,
                module_id,
            )?,
            body: block(required_node(node, "body", module_id)?, source, module_id)?,
        }),
        "pass_statement" => Ok(Stmt::Expr(Expr::Null)),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn block(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    named_children(node)
        .into_iter()
        .map(|n| stmt(n, source, module_id))
        .collect()
}

fn expr(node: Node<'_>, source: &str, module_id: &str) -> Result<Expr, FrontendError> {
    match node.kind() {
        "identifier" if text(node, source) == "print" => {
            Ok(Expr::Intrinsic(crate::ir::Intrinsic::Print))
        }
        "identifier" => Ok(Expr::Name(text(node, source).into())),
        "integer" | "float" => Ok(Expr::Number(text(node, source).replace('_', ""))),
        "string" | "concatenated_string" => Ok(Expr::String(unquote(text(node, source)))),
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        "none" => Ok(Expr::Null),
        "list" | "tuple" | "set" => Ok(Expr::Array(
            named_children(node)
                .into_iter()
                .map(|n| expr(n, source, module_id))
                .collect::<Result<_, _>>()?,
        )),
        "dictionary" => {
            let mut entries = vec![];
            for pair in named_children(node) {
                if pair.kind() != "pair" {
                    return Err(unsupported(LANG, module_id, pair));
                }
                entries.push((
                    expr(required_node(pair, "key", module_id)?, source, module_id)?,
                    expr(required_node(pair, "value", module_id)?, source, module_id)?,
                ));
            }
            Ok(Expr::Object(entries))
        }
        "comparison_operator" => {
            let values = named_children(node);
            if values.len() != 2 {
                return Err(unsupported(LANG, module_id, node));
            }
            let left = values[0];
            let right = values[1];
            let raw_op = &source[left.end_byte()..right.start_byte()];
            let op = binary_op(raw_op.trim()).ok_or_else(|| unsupported(LANG, module_id, node))?;
            Ok(Expr::Binary {
                op,
                left: Box::new(expr(left, source, module_id)?),
                right: Box::new(expr(right, source, module_id)?),
            })
        }
        "binary_operator" | "boolean_operator" => {
            let left = required_node(node, "left", module_id)?;
            let right = required_node(node, "right", module_id)?;
            let raw_op = &source[left.end_byte()..right.start_byte()];
            let op = binary_op(raw_op.trim()).ok_or_else(|| unsupported(LANG, module_id, node))?;
            Ok(Expr::Binary {
                op,
                left: Box::new(expr(left, source, module_id)?),
                right: Box::new(expr(right, source, module_id)?),
            })
        }
        "unary_operator" | "not_operator" => {
            let operand = node
                .child_by_field_name("argument")
                .or_else(|| node.named_child(0))
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            let op = unary_op(source[node.start_byte()..operand.start_byte()].trim())
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            Ok(Expr::Unary {
                op,
                operand: Box::new(expr(operand, source, module_id)?),
            })
        }
        "call" => {
            let callee = required_node(node, "function", module_id)?;
            let arguments = required_node(node, "arguments", module_id)?;
            Ok(Expr::Call {
                callee: Box::new(expr(callee, source, module_id)?),
                args: named_children(arguments)
                    .into_iter()
                    .map(|n| expr(n, source, module_id))
                    .collect::<Result<_, _>>()?,
            })
        }
        "attribute" => Ok(Expr::Member {
            object: Box::new(expr(
                required_node(node, "object", module_id)?,
                source,
                module_id,
            )?),
            property: Box::new(expr(
                required_node(node, "attribute", module_id)?,
                source,
                module_id,
            )?),
            computed: false,
        }),
        "subscript" => Ok(Expr::Member {
            object: Box::new(expr(
                required_node(node, "value", module_id)?,
                source,
                module_id,
            )?),
            property: Box::new(expr(
                required_node(node, "subscript", module_id)?,
                source,
                module_id,
            )?),
            computed: true,
        }),
        "parenthesized_expression" => expr(
            node.named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?,
            source,
            module_id,
        ),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn required_node<'a>(
    node: Node<'a>,
    field: &str,
    module_id: &str,
) -> Result<Node<'a>, FrontendError> {
    node.child_by_field_name(field)
        .ok_or_else(|| unsupported(LANG, module_id, node))
}

fn required_field<'a>(
    node: Node<'a>,
    field: &str,
    module_id: &str,
) -> Result<Node<'a>, FrontendError> {
    required_node(node, field, module_id)
}
