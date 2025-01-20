use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::cache::CacheConfig;
use super::domain::LocatedDomainConfig;
use super::edit::{ConfigEdit, ConfigEditor, EditError};
use super::external::ExternalDependencyConfig;
use super::interfaces::InterfaceConfig;
use super::modules::{deserialize_modules, serialize_modules, DependencyConfig, ModuleConfig};
use super::root_module::RootModuleTreatment;
use super::rules::RulesConfig;
use super::utils::*;

#[derive(Default, Clone)]
#[pyclass(get_all, module = "tach.extension")]
pub struct UnusedDependencies {
    pub path: String,
    pub dependencies: Vec<DependencyConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
#[pyclass(module = "tach.extension")]
pub struct ProjectConfig {
    #[serde(
        default,
        deserialize_with = "deserialize_modules",
        serialize_with = "serialize_modules"
    )]
    #[pyo3(get)]
    pub modules: Vec<ModuleConfig>,
    #[serde(default)]
    #[pyo3(get)]
    pub interfaces: Vec<InterfaceConfig>,
    #[serde(default, skip_serializing_if = "is_empty")]
    #[pyo3(get)]
    pub layers: Vec<String>,
    #[serde(default, skip_serializing_if = "CacheConfig::is_default")]
    #[pyo3(get)]
    pub cache: CacheConfig,
    #[serde(default, skip_serializing_if = "ExternalDependencyConfig::is_default")]
    #[pyo3(get)]
    pub external: ExternalDependencyConfig,
    #[serde(default)]
    #[pyo3(get)]
    pub exclude: Vec<String>,
    #[serde(default = "default_source_roots")]
    #[pyo3(get)]
    pub source_roots: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "is_false")]
    #[pyo3(get)]
    pub exact: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[pyo3(get)]
    pub disable_logging: bool,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    #[pyo3(get)]
    pub ignore_type_checking_imports: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[pyo3(get)]
    pub include_string_imports: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[pyo3(get)]
    pub forbid_circular_dependencies: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    #[pyo3(get)]
    pub use_regex_matching: bool,
    #[serde(default, skip_serializing_if = "RootModuleTreatment::is_default")]
    #[pyo3(get)]
    pub root_module: RootModuleTreatment,
    #[serde(default, skip_serializing_if = "RulesConfig::is_default")]
    #[pyo3(get)]
    pub rules: RulesConfig,
    #[serde(skip)]
    pub domains: Vec<LocatedDomainConfig>,
    #[serde(skip)]
    pub pending_edits: Vec<ConfigEdit>,
    // If location is None, the config is not on disk
    #[serde(skip)]
    pub location: Option<PathBuf>,
}

pub fn default_source_roots() -> Vec<PathBuf> {
    vec![PathBuf::from(".")]
}

pub const DEFAULT_EXCLUDE_PATHS: [&str; 5] = [
    "**/tests",
    "**/docs",
    "**/*__pycache__",
    "**/*egg-info",
    "**/venv",
];

pub fn default_excludes() -> Vec<String> {
    DEFAULT_EXCLUDE_PATHS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            // special defaults
            exclude: default_excludes(),
            source_roots: default_source_roots(),
            ignore_type_checking_imports: true,
            // normal defaults
            modules: Default::default(),
            interfaces: Default::default(),
            layers: Default::default(),
            cache: Default::default(),
            external: Default::default(),
            exact: Default::default(),
            disable_logging: Default::default(),
            include_string_imports: Default::default(),
            forbid_circular_dependencies: Default::default(),
            use_regex_matching: Default::default(),
            root_module: Default::default(),
            rules: Default::default(),
            domains: Default::default(),
            pending_edits: Default::default(),
            location: Default::default(),
        }
    }
}

impl ProjectConfig {
    pub fn dependencies_for_module(&self, module: &str) -> Option<&Vec<DependencyConfig>> {
        self.all_modules()
            .find(|mod_config| mod_config.path == module)
            .map(|mod_config| mod_config.depends_on.as_ref())?
    }

    pub fn set_location(&mut self, location: PathBuf) {
        self.location = Some(location);
    }

    // TODO: use location for this
    pub fn prepend_roots(&self, project_root: &Path) -> Vec<PathBuf> {
        // don't prepend if root is "."
        self.source_roots
            .iter()
            .map(|root| {
                if root.display().to_string() == "." {
                    project_root.to_path_buf()
                } else {
                    project_root.join(root)
                }
            })
            .collect()
    }

    pub fn with_dependencies_removed(&self) -> Self {
        let mut new_modules = self.modules.clone();
        new_modules.iter_mut().for_each(|module| {
            if let Some(depends_on) = &mut module.depends_on {
                depends_on.clear();
            }
        });
        Self {
            modules: new_modules,
            ..self.clone()
        }
    }

    pub fn add_domain(&mut self, domain: LocatedDomainConfig) {
        self.domains.push(domain);
    }

    pub fn all_modules(&self) -> impl Iterator<Item = &ModuleConfig> {
        self.modules
            .iter()
            .chain(self.domains.iter().flat_map(|domain| domain.modules()))
    }

    pub fn all_interfaces(&self) -> impl Iterator<Item = &InterfaceConfig> {
        self.interfaces
            .iter()
            .chain(self.domains.iter().flat_map(|domain| domain.interfaces()))
    }
}

impl ConfigEditor for ProjectConfig {
    fn enqueue_edit(&mut self, edit: &ConfigEdit) -> Result<(), EditError> {
        // Enqueue the edit for any relevant domains
        let domain_results = self
            .domains
            .iter_mut()
            .map(|domain| domain.enqueue_edit(edit))
            .collect::<Vec<Result<(), EditError>>>();

        let result = match edit {
            ConfigEdit::CreateModule { path } => {
                if !domain_results.iter().any(|r| r.is_ok()) {
                    if self.modules.iter().any(|module| module.path == *path) {
                        Err(EditError::ModuleAlreadyExists)
                    } else {
                        // If no domain will create the module, and the module doesn't already exist,
                        // enqueue the edit
                        self.pending_edits.push(edit.clone());
                        Ok(())
                    }
                } else {
                    Err(EditError::NotApplicable)
                }
            }
            ConfigEdit::DeleteModule { path }
            | ConfigEdit::MarkModuleAsUtility { path }
            | ConfigEdit::UnmarkModuleAsUtility { path }
            | ConfigEdit::AddDependency { path, .. }
            | ConfigEdit::RemoveDependency { path, .. } => {
                // If we know of this module, enqueue the edit
                if self.modules.iter().any(|module| module.path == *path) {
                    self.pending_edits.push(edit.clone());
                    Ok(())
                } else {
                    Err(EditError::ModuleNotFound)
                }
            }
            ConfigEdit::AddSourceRoot { .. } | ConfigEdit::RemoveSourceRoot { .. } => {
                // Source root edits are always applicable to project config
                self.pending_edits.push(edit.clone());
                Ok(())
            }
        };

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                // If any domain enqueued the edit, return Ok
                if domain_results.iter().any(|r| r.is_ok()) {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        }
    }

    fn apply_edits(&mut self) -> Result<(), EditError> {
        for domain in &mut self.domains {
            domain.apply_edits()?;
        }

        if self.pending_edits.is_empty() {
            return Ok(());
        }
        let config_path = self
            .location
            .as_ref()
            .ok_or(EditError::ConfigDoesNotExist)?;

        let toml_str =
            std::fs::read_to_string(config_path).map_err(|_| EditError::ConfigDoesNotExist)?;
        let mut doc = toml_str
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| EditError::ParsingFailed)?;

        for edit in &self.pending_edits {
            match edit {
                ConfigEdit::CreateModule { path } => {
                    let mut module_table = toml_edit::Table::new();
                    module_table.insert("path", toml_edit::value(path));
                    module_table.insert("depends_on", toml_edit::value(toml_edit::Array::new()));

                    let modules = doc["modules"]
                        .or_insert(toml_edit::Item::ArrayOfTables(Default::default()));
                    if let toml_edit::Item::ArrayOfTables(array) = modules {
                        array.push(module_table);
                    }
                }
                ConfigEdit::DeleteModule { path } => {
                    if let toml_edit::Item::ArrayOfTables(modules) = &mut doc["modules"] {
                        modules.retain(|table| {
                            table["path"].as_str().map(|p| p != path).unwrap_or(true)
                        });
                    }
                }
                ConfigEdit::MarkModuleAsUtility { path }
                | ConfigEdit::UnmarkModuleAsUtility { path } => {
                    if let toml_edit::Item::ArrayOfTables(modules) = &mut doc["modules"] {
                        for table in modules.iter_mut() {
                            if table["path"].as_str() == Some(path) {
                                match edit {
                                    ConfigEdit::MarkModuleAsUtility { .. } => {
                                        table.insert("utility", toml_edit::value(true));
                                    }
                                    ConfigEdit::UnmarkModuleAsUtility { .. } => {
                                        table.remove("utility");
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }
                }
                ConfigEdit::AddDependency { path, dependency }
                | ConfigEdit::RemoveDependency { path, dependency } => {
                    if let toml_edit::Item::ArrayOfTables(modules) = &mut doc["modules"] {
                        for table in modules.iter_mut() {
                            if table["path"].as_str() == Some(path) {
                                match edit {
                                    ConfigEdit::AddDependency { .. } => {
                                        let deps = table["depends_on"]
                                            .or_insert(toml_edit::value(toml_edit::Array::new()));
                                        if let toml_edit::Item::Value(toml_edit::Value::Array(
                                            array,
                                        )) = deps
                                        {
                                            array.push(dependency);
                                        }
                                    }
                                    ConfigEdit::RemoveDependency { .. } => {
                                        if let toml_edit::Item::Value(toml_edit::Value::Array(
                                            array,
                                        )) = &mut table["depends_on"]
                                        {
                                            array.retain(|dep| {
                                                dep.as_str()
                                                    .map(|d| d != dependency)
                                                    .unwrap_or(true)
                                            });
                                        }
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                    }
                }
                ConfigEdit::AddSourceRoot { filepath } => {
                    if let toml_edit::Item::Value(toml_edit::Value::Array(source_roots)) =
                        &mut doc["source_roots"]
                    {
                        if !source_roots.iter().any(|root| {
                            root.as_str() == Some(filepath.as_os_str().to_str().unwrap())
                        }) {
                            source_roots.push(filepath.display().to_string());
                        }
                    }
                }
                ConfigEdit::RemoveSourceRoot { filepath } => {
                    if let toml_edit::Item::Value(toml_edit::Value::Array(source_roots)) =
                        &mut doc["source_roots"]
                    {
                        source_roots.retain(|root| {
                            root.as_str()
                                .map(|s| s != filepath.as_os_str().to_str().unwrap())
                                .unwrap_or(true)
                        });
                    }
                }
            }
        }

        std::fs::write(config_path, doc.to_string()).map_err(|_| EditError::DiskWriteFailed)?;

        self.pending_edits.clear();
        Ok(())
    }
}

#[pymethods]
impl ProjectConfig {
    #[new]
    fn new() -> Self {
        ProjectConfig::default()
    }

    fn __str__(&self) -> String {
        format!("{:#?}", self)
    }

    fn serialize_json(&self) -> String {
        serde_json::to_string(&self).unwrap()
    }

    pub fn module_paths(&self) -> Vec<String> {
        self.all_modules()
            .map(|module| module.path.clone())
            .collect()
    }

    fn utility_paths(&self) -> Vec<String> {
        self.all_modules()
            .filter(|module| module.utility)
            .map(|module| module.path.clone())
            .collect()
    }

    pub fn create_module(&mut self, path: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::CreateModule { path })
    }

    pub fn delete_module(&mut self, path: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::DeleteModule { path })
    }

    pub fn mark_module_as_utility(&mut self, path: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::MarkModuleAsUtility { path })
    }

    pub fn unmark_module_as_utility(&mut self, path: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::UnmarkModuleAsUtility { path })
    }

    pub fn add_dependency(&mut self, path: String, dependency: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::AddDependency { path, dependency })
    }

    pub fn remove_dependency(&mut self, path: String, dependency: String) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::RemoveDependency { path, dependency })
    }

    pub fn add_source_root(&mut self, filepath: PathBuf) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::AddSourceRoot { filepath })
    }

    pub fn remove_source_root(&mut self, filepath: PathBuf) -> Result<(), EditError> {
        self.enqueue_edit(&ConfigEdit::RemoveSourceRoot { filepath })
    }

    pub fn save_edits(&mut self) -> Result<(), EditError> {
        self.apply_edits()
    }

    pub fn has_edits(&self) -> bool {
        !self.pending_edits.is_empty()
    }

    // TODO: only used in sync, probably should be removed
    pub fn with_modules(&self, modules: Vec<ModuleConfig>) -> Self {
        Self {
            modules,
            ..Clone::clone(self)
        }
    }

    pub fn set_modules(&mut self, module_paths: Vec<String>) {
        let new_module_paths: HashSet<String> = module_paths.into_iter().collect();
        let mut new_modules: Vec<ModuleConfig> = Vec::new();

        let mut original_modules_by_path: HashMap<String, ModuleConfig> = self
            .modules
            .drain(..)
            .map(|module| (module.path.clone(), module))
            .collect();

        for new_module_path in &new_module_paths {
            if let Some(mut original_module) = original_modules_by_path.remove(new_module_path) {
                if let Some(deps) = original_module.depends_on.as_mut() {
                    deps.retain(|dep| new_module_paths.contains(&dep.path))
                }
                new_modules.push(original_module);
            } else {
                new_modules.push(ModuleConfig {
                    path: new_module_path.to_string(),
                    ..Default::default()
                });
            }
        }

        self.modules = new_modules;
    }

    pub fn mark_utilities(&mut self, utility_paths: Vec<String>) {
        for module in &mut self.modules {
            module.utility = utility_paths.contains(&module.path);
        }
    }

    pub fn add_dependency_to_module(&mut self, module: &str, dependency: DependencyConfig) {
        if let Some(module_config) = self
            .modules
            .iter_mut()
            .find(|mod_config| mod_config.path == module)
        {
            match &mut module_config.depends_on {
                Some(depends_on) => {
                    if !depends_on.iter().any(|dep| dep.path == dependency.path) {
                        depends_on.push(dependency);
                    }
                }
                None => module_config.depends_on = Some(vec![dependency]),
            }
        } else {
            self.modules.push(ModuleConfig {
                path: module.to_string(),
                depends_on: Some(vec![dependency]),
                ..Default::default()
            });
        }
    }

    pub fn compare_dependencies(&self, other_config: &ProjectConfig) -> Vec<UnusedDependencies> {
        let mut all_unused_dependencies = Vec::new();
        let own_module_paths: HashSet<&String> =
            self.all_modules().map(|module| &module.path).collect();

        for module_config in other_config.all_modules() {
            if !own_module_paths.contains(&module_config.path) {
                all_unused_dependencies.push(UnusedDependencies {
                    path: module_config.path.clone(),
                    dependencies: module_config.depends_on.clone().unwrap_or_default(),
                });
                continue;
            }

            let own_module_dependency_paths: HashSet<&String> = self
                .dependencies_for_module(&module_config.path)
                .map(|deps| deps.iter().map(|dep| &dep.path).collect())
                .unwrap_or_default();

            let current_dependency_paths: HashSet<&String> = module_config
                .dependencies_iter()
                .map(|dep| &dep.path)
                .collect();

            let extra_dependency_paths: HashSet<&&String> = current_dependency_paths
                .difference(&own_module_dependency_paths)
                .collect();

            if !extra_dependency_paths.is_empty() {
                let extra_dependencies: Vec<DependencyConfig> = module_config
                    .dependencies_iter()
                    .filter(|dep| extra_dependency_paths.contains(&&dep.path))
                    .cloned()
                    .collect();

                all_unused_dependencies.push(UnusedDependencies {
                    path: module_config.path.clone(),
                    dependencies: extra_dependencies,
                });
            }
        }

        all_unused_dependencies
    }
}
