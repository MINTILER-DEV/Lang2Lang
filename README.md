# Lang2Lang

Lang2Lang is an early semantic source-to-source converter for Python, JavaScript, and Lua. Each frontend lowers a deliberately supported language subset into one serializable IR; each backend emits that IR in any of the three languages. Local imported modules are discovered and converted with the entry module.

This is a foundation, not a claim that arbitrary programs can be translated correctly. The languages disagree about truthiness, numbers, strings, table/object behavior, iteration, exceptions, async execution, classes, module initialization, and standard libraries. Lang2Lang currently rejects syntax outside its supported subset instead of copying source text or silently guessing.

## Try it

```sh
cargo run -- convert examples/python/main.py --to javascript --out build/js
cargo run -- convert examples/javascript/main.js --to lua --out build/lua
cargo run -- convert examples/lua/main.lua --to python --out build/python
cargo run -- ir examples/python/main.py
```

The converter infers the source language from `.py`, `.js`/`.mjs`/`.cjs`, or `.lua`. Targets accept `python`/`py`, `javascript`/`js`, and `lua`.

For JavaScript, only relative imports are treated as local. Python imports and Lua `require` calls are resolved from the entry module's directory. By default unresolved packages stop conversion, because retaining a Python package import in generated JavaScript would usually be wrong. `--allow-external` is available for experiments.

If the entry lives below the desired module root, pass `--project-root <directory>`. Generated JavaScript import paths are recalculated relative to each output module.

## Supported subset

- Named functions with identifier parameters
- Local/ordinary variables and assignment
- `return`, `if`/`else`, and `while`
- Null, booleans, basic decimal numbers, simple quoted strings, arrays/lists, and object/dictionary/table literals
- Calls, property/index access, unary `not`/`!`/`-`/`+`, and common arithmetic, comparison, and boolean operators
- Python `import`/`from`, JavaScript ESM imports, and Lua `local x = require("x")`
- Recursive conversion of locally available modules

Unsupported constructs produce a diagnostic naming the source construct and location. Notable next milestones are semantic type/effect analysis, language runtime shims, exports, loops/iteration, exceptions, classes/prototypes/metatables, async behavior, source maps, and package manifests.

## Architecture

```text
Python parser ─┐
JavaScript ────┼─> semantic IR ─┬─> Python emitter
Lua parser ────┘                 ├─> JavaScript emitter
                                 └─> Lua emitter

entry module -> dependency resolver -> parse every local module -> emit every module
```

The future C/C++/Rust track should use a separate typed, lower-level IR. It can share the CLI and project graph, but forcing dynamic-language object and truthiness semantics into a systems-language IR would make both conversion modes less reliable.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```
