use tree_sitter::{Node, Parser};

use crate::{
    Language,
    frontend::{FrontendError, shared::*},
    ir::{Expr, Import, ImportBinding, Module, Stmt},
};

const LANG: Language = Language::Lua;

pub fn parse(module_id: &str, source: &str) -> Result<Module, FrontendError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_lua::LANGUAGE.into())
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
        for statement in stmt_many(node, source, module_id)? {
            if let Some(import) = require_import(&statement) {
                module.imports.push(import);
            } else {
                module.body.push(statement);
            }
        }
    }
    let explicit_exports = take_module_exports(&mut module.body);
    module.exports = inferred_exports(&module.body);
    for name in explicit_exports {
        if !module.exports.contains(&name) {
            module.exports.push(name);
        }
    }
    Ok(module)
}

fn take_module_exports(body: &mut Vec<Stmt>) -> Vec<String> {
    let Some(Stmt::Return {
        value: Some(Expr::Object(entries)),
    }) = body.last()
    else {
        return vec![];
    };
    let exports: Option<Vec<_>> = entries
        .iter()
        .map(|(key, value)| match (key, value) {
            (Expr::String(exported), Expr::Name(local)) if exported == local => Some(local.clone()),
            _ => None,
        })
        .collect();
    if let Some(exports) = exports {
        body.pop();
        exports
    } else {
        vec![]
    }
}

fn require_import(statement: &Stmt) -> Option<Import> {
    let Stmt::Let {
        name,
        value: Some(Expr::Call { callee, args }),
        ..
    } = statement
    else {
        return None;
    };
    if !matches!(callee.as_ref(), Expr::Name(n) if n == "require") {
        return None;
    }
    let [Expr::String(source)] = args.as_slice() else {
        return None;
    };
    Some(Import {
        source: source.replace('.', "/"),
        bindings: vec![ImportBinding {
            imported: "*".into(),
            local: name.clone(),
        }],
        external: false,
    })
}

fn stmt_many(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    if node.kind() == "variable_declaration" {
        let inner = node
            .named_child(0)
            .ok_or_else(|| unsupported(LANG, module_id, node))?;
        if inner.kind() == "assignment_statement" {
            return assignment(inner, source, module_id, true);
        }
        let names = variables(inner, source, module_id)?;
        return Ok(names
            .into_iter()
            .map(|name| Stmt::Let {
                name,
                value: None,
                mutable: true,
            })
            .collect());
    }
    if node.kind() == "assignment_statement" {
        return assignment(node, source, module_id, false);
    }
    Ok(vec![stmt(node, source, module_id)?])
}

fn assignment(
    node: Node<'_>,
    source: &str,
    module_id: &str,
    local: bool,
) -> Result<Vec<Stmt>, FrontendError> {
    let children = named_children(node);
    let vars = children
        .iter()
        .find(|n| n.kind() == "variable_list")
        .ok_or_else(|| unsupported(LANG, module_id, node))?;
    let names = variable_nodes(*vars);
    let values: Vec<_> = children
        .iter()
        .find(|n| n.kind() == "expression_list")
        .map(|n| named_children(*n))
        .unwrap_or_default();
    if names.len() != values.len() && !values.is_empty() {
        return Err(unsupported(LANG, module_id, node));
    }
    names
        .into_iter()
        .enumerate()
        .map(|(i, target)| {
            let value = values
                .get(i)
                .copied()
                .map(|n| expr(n, source, module_id))
                .transpose()?;
            if local {
                if target.kind() != "identifier" {
                    return Err(unsupported(LANG, module_id, target));
                }
                Ok(Stmt::Let {
                    name: text(target, source).into(),
                    value,
                    mutable: true,
                })
            } else {
                Ok(Stmt::Assign {
                    target: expr(target, source, module_id)?,
                    value: value.ok_or_else(|| unsupported(LANG, module_id, node))?,
                })
            }
        })
        .collect()
}

fn stmt(node: Node<'_>, source: &str, module_id: &str) -> Result<Stmt, FrontendError> {
    match node.kind() {
        "function_declaration" => {
            let name = required(node, "name", module_id)?;
            if name.kind() != "identifier" {
                return Err(unsupported(LANG, module_id, name));
            }
            let params = named_children(required(node, "parameters", module_id)?)
                .into_iter()
                .filter(|n| n.kind() == "identifier")
                .map(|n| text(n, source).into())
                .collect();
            let body = node
                .child_by_field_name("body")
                .map(|b| block(b, source, module_id))
                .transpose()?
                .unwrap_or_default();
            Ok(Stmt::Function {
                name: text(name, source).into(),
                params,
                body,
            })
        }
        "function_call" => Ok(Stmt::Expr(expr(node, source, module_id)?)),
        "return_statement" => {
            let values = node.named_child(0).map(named_children).unwrap_or_default();
            if values.len() > 1 {
                return Err(unsupported(LANG, module_id, node));
            }
            Ok(Stmt::Return {
                value: values
                    .first()
                    .copied()
                    .map(|n| expr(n, source, module_id))
                    .transpose()?,
            })
        }
        "if_statement" | "elseif_statement" => parse_if(node, source, module_id),
        "while_statement" => Ok(Stmt::While {
            condition: expr(required(node, "condition", module_id)?, source, module_id)?,
            body: node
                .child_by_field_name("body")
                .map(|b| block(b, source, module_id))
                .transpose()?
                .unwrap_or_default(),
        }),
        "empty_statement" => Ok(Stmt::Expr(Expr::Null)),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn parse_if(node: Node<'_>, source: &str, module_id: &str) -> Result<Stmt, FrontendError> {
    let condition = expr(required(node, "condition", module_id)?, source, module_id)?;
    let then_body = node
        .child_by_field_name(if node.kind() == "if_statement" {
            "consequence"
        } else {
            "body"
        })
        .map(|b| block(b, source, module_id))
        .transpose()?
        .unwrap_or_default();
    let mut alternative = vec![];
    for child in named_children(node) {
        match child.kind() {
            "elseif_statement" => {
                alternative = vec![parse_if(child, source, module_id)?];
                break;
            }
            "else_statement" => {
                alternative = child
                    .child_by_field_name("body")
                    .map(|b| block(b, source, module_id))
                    .transpose()?
                    .unwrap_or_default();
                break;
            }
            _ => {}
        }
    }
    Ok(Stmt::If {
        condition,
        then_body,
        else_body: alternative,
    })
}

fn block(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<Stmt>, FrontendError> {
    let mut out = vec![];
    for child in named_children(node) {
        out.extend(stmt_many(child, source, module_id)?);
    }
    Ok(out)
}

fn expr(node: Node<'_>, source: &str, module_id: &str) -> Result<Expr, FrontendError> {
    match node.kind() {
        "variable" => expr(
            node.named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?,
            source,
            module_id,
        ),
        "identifier" if text(node, source) == "print" => {
            Ok(Expr::Intrinsic(crate::ir::Intrinsic::Print))
        }
        "identifier" => Ok(Expr::Name(text(node, source).into())),
        "number" => Ok(Expr::Number(text(node, source).into())),
        "string" => {
            let raw = text(node, source);
            Ok(Expr::String(
                if raw.starts_with("[[") && raw.ends_with("]]") {
                    raw[2..raw.len() - 2].into()
                } else {
                    unquote(raw)
                },
            ))
        }
        "true" => Ok(Expr::Bool(true)),
        "false" => Ok(Expr::Bool(false)),
        "nil" => Ok(Expr::Null),
        "binary_expression" => {
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
        "unary_expression" => {
            let operand = required(node, "operand", module_id)?;
            let op = unary_op(source[node.start_byte()..operand.start_byte()].trim())
                .ok_or_else(|| unsupported(LANG, module_id, node))?;
            Ok(Expr::Unary {
                op,
                operand: Box::new(expr(operand, source, module_id)?),
            })
        }
        "function_call" => {
            let args = required(node, "arguments", module_id)?;
            Ok(Expr::Call {
                callee: Box::new(expr(required(node, "name", module_id)?, source, module_id)?),
                args: named_children(args)
                    .into_iter()
                    .flat_map(|n| {
                        if n.kind() == "expression_list" {
                            named_children(n)
                        } else {
                            vec![n]
                        }
                    })
                    .map(|n| expr(n, source, module_id))
                    .collect::<Result<_, _>>()?,
            })
        }
        "dot_index_expression" => Ok(Expr::Member {
            object: Box::new(expr(
                required(node, "table", module_id)?,
                source,
                module_id,
            )?),
            property: Box::new(Expr::Name(
                text(required(node, "field", module_id)?, source).into(),
            )),
            computed: false,
        }),
        "bracket_index_expression" => Ok(Expr::Member {
            object: Box::new(expr(
                required(node, "table", module_id)?,
                source,
                module_id,
            )?),
            property: Box::new(expr(
                required(node, "field", module_id)?,
                source,
                module_id,
            )?),
            computed: true,
        }),
        "table_constructor" => table(node, source, module_id),
        "parenthesized_expression" => expr(
            node.named_child(0)
                .ok_or_else(|| unsupported(LANG, module_id, node))?,
            source,
            module_id,
        ),
        _ => Err(unsupported(LANG, module_id, node)),
    }
}

fn table(node: Node<'_>, source: &str, module_id: &str) -> Result<Expr, FrontendError> {
    let fields = named_children(node);
    if fields
        .iter()
        .all(|f| f.child_by_field_name("name").is_none())
    {
        return Ok(Expr::Array(
            fields
                .into_iter()
                .map(|f| expr(required(f, "value", module_id)?, source, module_id))
                .collect::<Result<_, _>>()?,
        ));
    }
    let mut values = vec![];
    for field in fields {
        let name = required(field, "name", module_id)?;
        let key = if name.kind() == "identifier" {
            Expr::String(text(name, source).into())
        } else {
            expr(name, source, module_id)?
        };
        values.push((
            key,
            expr(required(field, "value", module_id)?, source, module_id)?,
        ));
    }
    Ok(Expr::Object(values))
}

fn variables(node: Node<'_>, source: &str, module_id: &str) -> Result<Vec<String>, FrontendError> {
    variable_nodes(node)
        .into_iter()
        .map(|n| {
            if n.kind() == "identifier" {
                Ok(text(n, source).into())
            } else {
                Err(unsupported(LANG, module_id, n))
            }
        })
        .collect()
}

fn variable_nodes(node: Node<'_>) -> Vec<Node<'_>> {
    named_children(node)
        .into_iter()
        .map(|n| {
            if n.kind() == "variable" {
                n.named_child(0).unwrap_or(n)
            } else {
                n
            }
        })
        .filter(|n| n.kind() != "attribute")
        .collect()
}

fn required<'a>(node: Node<'a>, field: &str, module_id: &str) -> Result<Node<'a>, FrontendError> {
    node.child_by_field_name(field)
        .ok_or_else(|| unsupported(LANG, module_id, node))
}
