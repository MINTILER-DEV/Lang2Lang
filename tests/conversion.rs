use std::{fs, path::Path};

use lang2lang::{ConvertOptions, Language, convert_project, frontend};

#[test]
fn parses_all_three_languages_into_the_same_shape() {
    let python = frontend::parse(
        Language::Python,
        "sample",
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    let javascript = frontend::parse(
        Language::JavaScript,
        "sample",
        "function add(a, b) { return a + b; }\n",
    )
    .unwrap();
    let lua = frontend::parse(
        Language::Lua,
        "sample",
        "function add(a, b)\n  return a + b\nend\n",
    )
    .unwrap();
    assert_eq!(python.body, javascript.body);
    assert_eq!(javascript.body, lua.body);
}

#[test]
fn converts_python_entry_and_local_dependency() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("main.py"),
        "from util import double\nanswer = double(21)\n",
    )
    .unwrap();
    fs::write(
        source.path().join("util.py"),
        "def double(x):\n    return x * 2\n",
    )
    .unwrap();
    let conversion = convert_project(&ConvertOptions {
        entry: source.path().join("main.py"),
        project_root: None,
        target: Language::JavaScript,
        allow_external: false,
    })
    .unwrap();
    assert_eq!(conversion.modules.len(), 2);
    assert!(conversion.files[Path::new("main.js")].contains("from \"./util.js\""));
    assert!(conversion.files[Path::new("util.js")].contains("function double(x)"));
}

#[test]
fn rejects_unsupported_constructs() {
    let error = frontend::parse(
        Language::Python,
        "sample",
        "for x in values:\n    print(x)\n",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported python syntax `for_statement`")
    );
}

#[test]
fn normalizes_print_across_languages() {
    let python = frontend::parse(Language::Python, "sample", "print(\"hi\")\n").unwrap();
    let javascript =
        frontend::parse(Language::JavaScript, "sample", "console.log(\"hi\");\n").unwrap();
    let lua = frontend::parse(Language::Lua, "sample", "print(\"hi\")\n").unwrap();
    assert_eq!(python.body, javascript.body);
    assert_eq!(javascript.body, lua.body);
}

#[test]
fn converts_javascript_module_graph_to_python() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("main.js"),
        "import { double } from './util.js';\nconst answer = double(21);\n",
    )
    .unwrap();
    fs::write(
        source.path().join("util.js"),
        "export function double(x) { return x * 2; }\n",
    )
    .unwrap();
    let conversion = convert_project(&ConvertOptions {
        entry: source.path().join("main.js"),
        project_root: None,
        target: Language::Python,
        allow_external: false,
    })
    .unwrap();
    assert!(
        conversion.files[Path::new("main.py")].contains("from util import double"),
        "{}",
        conversion.files[Path::new("main.py")]
    );
    assert!(conversion.files[Path::new("main.py")].contains("answer = double(21)"));
}

#[test]
fn converts_lua_require_graph_to_javascript() {
    let source = tempfile::tempdir().unwrap();
    fs::write(
        source.path().join("main.lua"),
        "local util = require(\"util\")\nlocal answer = util.double(21)\n",
    )
    .unwrap();
    fs::write(
        source.path().join("util.lua"),
        "function double(x)\n  return x * 2\nend\n",
    )
    .unwrap();
    let conversion = convert_project(&ConvertOptions {
        entry: source.path().join("main.lua"),
        project_root: None,
        target: Language::JavaScript,
        allow_external: false,
    })
    .unwrap();
    assert!(conversion.files[Path::new("main.js")].contains("import * as util"));
    assert!(conversion.files[Path::new("util.js")].contains("export function double(x)"));
}

#[test]
fn rewrites_nested_javascript_imports_relative_to_each_output() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("sub")).unwrap();
    fs::write(
        source.path().join("sub/main.js"),
        "import { double } from '../util.js';\nconsole.log(double(2));\n",
    )
    .unwrap();
    fs::write(
        source.path().join("util.js"),
        "export function double(x) { return x * 2; }\n",
    )
    .unwrap();
    let conversion = convert_project(&ConvertOptions {
        entry: source.path().join("sub/main.js"),
        project_root: Some(source.path().to_path_buf()),
        target: Language::JavaScript,
        allow_external: false,
    })
    .unwrap();
    assert!(conversion.files[Path::new("sub/main.js")].contains("from \"../util.js\""));
}

#[test]
fn recognizes_a_lua_return_table_as_module_exports() {
    let module = frontend::parse(
        Language::Lua,
        "math_utils",
        "function double(x)\n  return x * 2\nend\nreturn { double = double }\n",
    )
    .unwrap();
    assert_eq!(module.exports, ["double"]);
    assert!(!matches!(
        module.body.last(),
        Some(lang2lang::ir::Stmt::Return { .. })
    ));
}
