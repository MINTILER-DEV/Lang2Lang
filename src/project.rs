use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{Language, backend, frontend, ir::Module};

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub entry: PathBuf,
    pub project_root: Option<PathBuf>,
    pub target: Language,
    pub allow_external: bool,
}

#[derive(Debug)]
pub struct Conversion {
    pub modules: Vec<Module>,
    pub files: BTreeMap<PathBuf, String>,
}

pub fn convert_project(options: &ConvertOptions) -> Result<Conversion> {
    let entry = options
        .entry
        .canonicalize()
        .with_context(|| format!("could not open entry module {}", options.entry.display()))?;
    let root = match &options.project_root {
        Some(root) => root
            .canonicalize()
            .with_context(|| format!("could not open project root {}", root.display()))?,
        None => entry
            .parent()
            .context("entry module has no parent directory")?
            .to_path_buf(),
    };
    entry.strip_prefix(&root).with_context(|| {
        format!(
            "entry module {} is outside project root {}",
            entry.display(),
            root.display()
        )
    })?;
    let mut pending = vec![entry];
    let mut visited = HashSet::new();
    let mut modules = vec![];

    while let Some(path) = pending.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let language = Language::from_path(&path)
            .with_context(|| format!("cannot infer language from {}", path.display()))?;
        let relative = path.strip_prefix(&root).with_context(|| {
            format!(
                "module {} escapes project root {}",
                path.display(),
                root.display()
            )
        })?;
        let module_id = strip_extension(relative)
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let mut module = frontend::parse(language, &module_id, &source)?;

        for import in &mut module.imports {
            match resolve_import(&root, &path, language, &import.source)? {
                Some(dependency) => {
                    let dependency = dependency.canonicalize()?;
                    let relative = dependency.strip_prefix(&root).with_context(|| {
                        format!("dependency {} escapes project root", dependency.display())
                    })?;
                    import.source = strip_extension(relative)
                        .to_string_lossy()
                        .replace('\\', "/");
                    pending.push(dependency);
                }
                None if options.allow_external => import.external = true,
                None => bail!(
                    "external module `{}` imported by {} cannot be converted; provide its source locally or use --allow-external",
                    import.source,
                    path.display()
                ),
            }
        }
        modules.push(module);
    }
    modules.sort_by(|a, b| a.id.cmp(&b.id));
    let files = modules
        .iter()
        .map(|module| {
            let output = PathBuf::from(&module.id).with_extension(options.target.extension());
            (output, backend::emit(module, options.target))
        })
        .collect();
    Ok(Conversion { modules, files })
}

impl Conversion {
    pub fn write(&self, output_dir: &Path) -> Result<()> {
        for (relative, contents) in &self.files {
            let path = output_dir.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("could not create {}", parent.display()))?;
            }
            fs::write(&path, contents)
                .with_context(|| format!("could not write {}", path.display()))?;
        }
        Ok(())
    }
}

fn resolve_import(
    root: &Path,
    importer: &Path,
    language: Language,
    source: &str,
) -> Result<Option<PathBuf>> {
    let source_path = Path::new(source);
    if source_path.is_absolute() {
        return Ok(None);
    }
    let bases: Vec<PathBuf> = match language {
        Language::Python if source.starts_with('.') => {
            vec![importer.parent().unwrap_or(root).join(source_path)]
        }
        Language::Python | Language::Lua => vec![root.join(source_path)],
        Language::JavaScript if source.starts_with('.') => {
            vec![importer.parent().unwrap_or(root).join(source_path)]
        }
        Language::JavaScript => return Ok(None),
    };
    let extension = language.extension();
    for base in bases {
        let candidates = if base.extension().is_some() {
            vec![base]
        } else {
            vec![
                base.with_extension(extension),
                base.join(format!("init.{extension}")),
                base.join(format!("index.{extension}")),
            ]
        };
        for candidate in candidates {
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

fn strip_extension(path: &Path) -> PathBuf {
    let mut path = path.to_path_buf();
    path.set_extension("");
    path
}
