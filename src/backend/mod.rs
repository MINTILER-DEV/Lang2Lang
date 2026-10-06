use crate::{
    Language,
    ir::{BinaryOp, Expr, Import, Intrinsic, Module, Stmt, UnaryOp},
};

pub fn emit(module: &Module, target: Language) -> String {
    let mut emitter = Emitter {
        target,
        out: String::new(),
        indent: 0,
        module_id: module.id.clone(),
    };
    emitter.module(module);
    emitter.out
}

struct Emitter {
    target: Language,
    out: String,
    indent: usize,
    module_id: String,
}

impl Emitter {
    fn module(&mut self, module: &Module) {
        for import in &module.imports {
            self.import(import);
        }
        if !module.imports.is_empty() && !module.body.is_empty() {
            self.out.push('\n');
        }
        for statement in &module.body {
            self.stmt(statement, true, &module.exports);
        }
        if self.target == Language::Lua && !module.exports.is_empty() {
            self.out.push('\n');
            let exports = module
                .exports
                .iter()
                .map(|name| format!("{name} = {name}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.line(&format!("return {{{exports}}}"));
        }
    }

    fn import(&mut self, import: &Import) {
        let source = self.import_path(import);
        match self.target {
            Language::Python => {
                for binding in &import.bindings {
                    if binding.imported == "*" {
                        self.line(&format!(
                            "import {} as {}",
                            source.replace('/', "."),
                            binding.local
                        ));
                    } else if binding.imported == "default" {
                        self.line(&format!(
                            "from {} import default as {}",
                            source.replace('/', "."),
                            binding.local
                        ));
                    } else {
                        let alias = if binding.imported == binding.local {
                            String::new()
                        } else {
                            format!(" as {}", binding.local)
                        };
                        self.line(&format!(
                            "from {} import {}{}",
                            source.replace('/', "."),
                            binding.imported,
                            alias
                        ));
                    }
                }
            }
            Language::JavaScript => {
                for binding in &import.bindings {
                    if binding.imported == "*" {
                        self.line(&format!(
                            "import * as {} from {};",
                            binding.local,
                            quote(&source)
                        ));
                    } else if binding.imported == "default" {
                        self.line(&format!(
                            "import {} from {};",
                            binding.local,
                            quote(&source)
                        ));
                    } else {
                        let name = if binding.imported == binding.local {
                            binding.imported.clone()
                        } else {
                            format!("{} as {}", binding.imported, binding.local)
                        };
                        self.line(&format!("import {{ {name} }} from {};", quote(&source)));
                    }
                }
                if import.bindings.is_empty() {
                    self.line(&format!("import {};", quote(&source)));
                }
            }
            Language::Lua => {
                for binding in &import.bindings {
                    let module = format!("require({})", quote(&source.replace('/', ".")));
                    let value = if binding.imported == "*" || binding.imported == "default" {
                        module
                    } else {
                        format!("{module}.{}", binding.imported)
                    };
                    self.line(&format!("local {} = {value}", binding.local));
                }
                if import.bindings.is_empty() {
                    self.line(&format!("require({})", quote(&source.replace('/', "."))));
                }
            }
        }
    }

    fn import_path(&self, import: &Import) -> String {
        let source = &import.source;
        if import.external {
            return source.clone();
        }
        match self.target {
            Language::Python => source.trim_end_matches("/__init__").into(),
            Language::JavaScript => {
                let from: Vec<_> = self.module_id.split('/').collect();
                let to: Vec<_> = source.split('/').collect();
                let from_dir = &from[..from.len().saturating_sub(1)];
                let common = from_dir
                    .iter()
                    .zip(&to)
                    .take_while(|(left, right)| left == right)
                    .count();
                let mut parts = vec![".."; from_dir.len() - common];
                parts.extend_from_slice(&to[common..]);
                let joined = parts.join("/");
                let mut path = if joined.starts_with('.') {
                    joined
                } else {
                    format!("./{joined}")
                };
                if !path.ends_with(".js") {
                    path.push_str(".js");
                }
                path
            }
            Language::Lua => source.trim_end_matches("/init").into(),
        }
    }

    fn stmt(&mut self, statement: &Stmt, top_level: bool, exports: &[String]) {
        match statement {
            Stmt::Function { name, params, body } => match self.target {
                Language::Python => {
                    self.line(&format!("def {name}({}):", params.join(", ")));
                    self.nested(body, true);
                }
                Language::JavaScript => {
                    let export = if top_level && exports.contains(name) {
                        "export "
                    } else {
                        ""
                    };
                    self.line(&format!(
                        "{export}function {name}({}) {{",
                        params.join(", ")
                    ));
                    self.nested(body, false);
                    self.line("}");
                }
                Language::Lua => {
                    self.line(&format!("function {name}({})", params.join(", ")));
                    self.nested(body, false);
                    self.line("end");
                }
            },
            Stmt::Let {
                name,
                value,
                mutable,
            } => {
                let value = value
                    .as_ref()
                    .map(|v| self.expr(v))
                    .unwrap_or_else(|| self.null().into());
                match self.target {
                    Language::Python => self.line(&format!("{name} = {value}")),
                    Language::JavaScript => self.line(&format!(
                        "{}{} {name} = {value};",
                        if top_level && exports.contains(name) {
                            "export "
                        } else {
                            ""
                        },
                        if *mutable { "let" } else { "const" }
                    )),
                    Language::Lua => self.line(&format!("local {name} = {value}")),
                }
            }
            Stmt::Assign { target, value } => {
                let code = format!(
                    "{} = {}{}",
                    self.expr(target),
                    self.expr(value),
                    if self.target == Language::JavaScript {
                        ";"
                    } else {
                        ""
                    }
                );
                self.line(&code);
            }
            Stmt::Return { value } => {
                let suffix = value
                    .as_ref()
                    .map(|v| format!(" {}", self.expr(v)))
                    .unwrap_or_default();
                self.line(&format!(
                    "return{suffix}{}",
                    if self.target == Language::JavaScript {
                        ";"
                    } else {
                        ""
                    }
                ));
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let condition = self.expr(condition);
                match self.target {
                    Language::Python => {
                        self.line(&format!("if {condition}:"));
                        self.nested(then_body, true);
                        if !else_body.is_empty() {
                            self.line("else:");
                            self.nested(else_body, true);
                        }
                    }
                    Language::JavaScript => {
                        self.line(&format!("if ({condition}) {{"));
                        self.nested(then_body, false);
                        if else_body.is_empty() {
                            self.line("}");
                        } else {
                            self.line("} else {");
                            self.nested(else_body, false);
                            self.line("}");
                        }
                    }
                    Language::Lua => {
                        self.line(&format!("if {condition} then"));
                        self.nested(then_body, false);
                        if !else_body.is_empty() {
                            self.line("else");
                            self.nested(else_body, false);
                        }
                        self.line("end");
                    }
                }
            }
            Stmt::While { condition, body } => {
                let condition = self.expr(condition);
                match self.target {
                    Language::Python => {
                        self.line(&format!("while {condition}:"));
                        self.nested(body, true);
                    }
                    Language::JavaScript => {
                        self.line(&format!("while ({condition}) {{"));
                        self.nested(body, false);
                        self.line("}");
                    }
                    Language::Lua => {
                        self.line(&format!("while {condition} do"));
                        self.nested(body, false);
                        self.line("end");
                    }
                }
            }
            Stmt::Expr(value) => {
                if !matches!(value, Expr::Null) {
                    let suffix = if self.target == Language::JavaScript {
                        ";"
                    } else {
                        ""
                    };
                    let rendered = self.expr(value);
                    self.line(&format!("{rendered}{suffix}"));
                }
            }
        }
    }

    fn expr(&self, expression: &Expr) -> String {
        match expression {
            Expr::Null => self.null().into(),
            Expr::Bool(value) => match self.target {
                Language::Python => if *value { "True" } else { "False" }.into(),
                _ => value.to_string(),
            },
            Expr::Number(value) => value.clone(),
            Expr::String(value) => quote(value),
            Expr::Name(value) => value.clone(),
            Expr::Intrinsic(Intrinsic::Print) => match self.target {
                Language::JavaScript => "console.log".into(),
                Language::Python | Language::Lua => "print".into(),
            },
            Expr::Array(values) => {
                let values = values
                    .iter()
                    .map(|v| self.expr(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                match self.target {
                    Language::Lua => format!("{{{values}}}"),
                    _ => format!("[{values}]"),
                }
            }
            Expr::Object(values) => {
                let values = values
                    .iter()
                    .map(|(k, v)| match self.target {
                        Language::Python => format!("{}: {}", self.expr(k), self.expr(v)),
                        Language::JavaScript => format!("[{}]: {}", self.expr(k), self.expr(v)),
                        Language::Lua => format!("[{}] = {}", self.expr(k), self.expr(v)),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{values}}}")
            }
            Expr::Unary { op, operand } => {
                let op = match (self.target, op) {
                    (Language::Python | Language::Lua, UnaryOp::Not) => "not ",
                    (_, UnaryOp::Not) => "!",
                    (_, UnaryOp::Negate) => "-",
                    (_, UnaryOp::Positive) => "+",
                };
                format!("({op}{})", self.expr(operand))
            }
            Expr::Binary { op, left, right } => {
                if *op == BinaryOp::FloorDivide && self.target == Language::JavaScript {
                    return format!("Math.floor({} / {})", self.expr(left), self.expr(right));
                }
                let op = operator(self.target, *op);
                format!("({} {op} {})", self.expr(left), self.expr(right))
            }
            Expr::Call { callee, args } => format!(
                "{}({})",
                self.expr(callee),
                args.iter()
                    .map(|a| self.expr(a))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Expr::Member {
                object,
                property,
                computed,
            } => {
                if *computed {
                    format!("{}[{}]", self.expr(object), self.expr(property))
                } else {
                    format!("{}.{}", self.expr(object), self.expr(property))
                }
            }
        }
    }

    fn nested(&mut self, body: &[Stmt], python_nonempty: bool) {
        self.indent += 1;
        if body.is_empty() && python_nonempty {
            self.line("pass");
        }
        for statement in body {
            self.stmt(statement, false, &[]);
        }
        self.indent -= 1;
    }

    fn null(&self) -> &'static str {
        match self.target {
            Language::Python => "None",
            Language::JavaScript => "null",
            Language::Lua => "nil",
        }
    }
    fn line(&mut self, value: &str) {
        self.out.push_str(&"    ".repeat(self.indent));
        self.out.push_str(value);
        self.out.push('\n');
    }
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("strings are serializable")
}

fn operator(language: Language, op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::FloorDivide => "//",
        BinaryOp::Modulo => "%",
        BinaryOp::Power => {
            if language == Language::Lua {
                "^"
            } else {
                "**"
            }
        }
        BinaryOp::Equal => {
            if language == Language::JavaScript {
                "==="
            } else {
                "=="
            }
        }
        BinaryOp::NotEqual => {
            if language == Language::JavaScript {
                "!=="
            } else if language == Language::Lua {
                "~="
            } else {
                "!="
            }
        }
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::And => {
            if language == Language::JavaScript {
                "&&"
            } else {
                "and"
            }
        }
        BinaryOp::Or => {
            if language == Language::JavaScript {
                "||"
            } else {
                "or"
            }
        }
    }
}
