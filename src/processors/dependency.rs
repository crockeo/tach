use std::path::PathBuf;
use std::sync::Arc;

use ruff_text_size::TextSize;

use crate::config::plugins::django::DjangoConfig;
use crate::config::root_module::RootModuleTreatment;
use crate::config::ProjectConfig;
use crate::diagnostics::{FileProcessor, Result as DiagnosticResult};
use crate::filesystem::{self, ProjectFile};
use crate::modules::error::ModuleTreeError;
use crate::modules::{ModuleNode, ModuleTree};
use crate::python::parsing::parse_python_source;

use super::django::fkey::{get_foreign_key_references, get_known_apps};
use super::file_module::FileModule;
use super::import::{get_normalized_imports, get_normalized_imports_from_ast, NormalizedImport};
use super::reference::SourceCodeReference;

#[derive(Debug)]
pub enum Dependency {
    Import(NormalizedImport),
    Reference(SourceCodeReference),
}

impl Dependency {
    pub fn module_path(&self) -> &str {
        match self {
            Dependency::Import(import) => &import.module_path,
            Dependency::Reference(reference) => &reference.module_path,
        }
    }

    pub fn offset(&self) -> TextSize {
        match self {
            Dependency::Import(import) => import.alias_offset,
            Dependency::Reference(reference) => reference.offset,
        }
    }
}

impl From<NormalizedImport> for Dependency {
    fn from(normalized_import: NormalizedImport) -> Self {
        Dependency::Import(normalized_import)
    }
}

impl From<SourceCodeReference> for Dependency {
    fn from(source_code_reference: SourceCodeReference) -> Self {
        Dependency::Reference(source_code_reference)
    }
}

#[derive(Debug)]
pub struct DjangoMetadata<'a> {
    pub config: &'a DjangoConfig,
    pub known_apps: Vec<String>,
}

impl<'a> DjangoMetadata<'a> {
    pub fn new(source_roots: &[PathBuf], django_config: &'a DjangoConfig) -> Self {
        let known_apps = get_known_apps(source_roots, django_config).unwrap_or_default();
        Self {
            config: django_config,
            known_apps,
        }
    }
}

#[derive(Debug)]
pub struct InternalDependencyExtractor<'a> {
    module_tree: &'a ModuleTree,
    source_roots: &'a [PathBuf],
    project_config: &'a ProjectConfig,
    django_metadata: Option<DjangoMetadata<'a>>,
}

impl<'a> InternalDependencyExtractor<'a> {
    pub fn new(
        source_roots: &'a [PathBuf],
        module_tree: &'a ModuleTree,
        project_config: &'a ProjectConfig,
    ) -> Self {
        let django_metadata = project_config
            .plugins
            .django
            .as_ref()
            .map(|django_config| DjangoMetadata::new(source_roots, django_config));

        Self {
            source_roots,
            module_tree,
            project_config,
            django_metadata,
        }
    }
}

impl<'a> FileProcessor<'a, ProjectFile<'a>> for InternalDependencyExtractor<'a> {
    type ProcessedFile = FileModule<'a>;

    fn process(&self, file_path: ProjectFile<'a>) -> DiagnosticResult<Self::ProcessedFile> {
        let mod_path = filesystem::file_to_module_path(self.source_roots, file_path.as_ref())?;
        let module = self
            .module_tree
            .find_nearest(mod_path.as_ref())
            .ok_or(ModuleTreeError::ModuleNotFound(mod_path))?;

        if module.is_unchecked() {
            return Ok(FileModule::new(file_path, module));
        }

        if module.is_root() && self.project_config.root_module == RootModuleTreatment::Ignore {
            return Ok(FileModule::new(file_path, module));
        }

        let mut file_module = FileModule::new(file_path, module);
        let mut dependencies: Vec<Dependency> = vec![];
        let file_ast = parse_python_source(file_module.contents())?;

        let project_imports = get_normalized_imports_from_ast(
            self.source_roots,
            file_module.file_path(),
            &file_ast,
            self.project_config.ignore_type_checking_imports,
            self.project_config.include_string_imports,
        )?
        .into_iter()
        .filter_map(|import| {
            if filesystem::is_project_import(self.source_roots, &import.module_path) {
                Some(Dependency::Import(import))
            } else {
                // Remove directives that match irrelevant imports
                file_module
                    .ignore_directives
                    .remove_matching_directives(file_module.line_number(import.import_offset));
                None
            }
        });
        dependencies.extend(project_imports);

        if self.django_metadata.is_some() {
            dependencies.extend(
                get_foreign_key_references(&file_ast)
                    .into_iter()
                    .map(Dependency::Reference),
            );
        }

        file_module.extend_dependencies(dependencies);
        Ok(file_module)
    }
}

#[derive(Debug)]
pub struct ExternalDependencyExtractor<'a> {
    source_roots: &'a [PathBuf],
    project_config: &'a ProjectConfig,
}

impl<'a> ExternalDependencyExtractor<'a> {
    pub fn new(source_roots: &'a [PathBuf], project_config: &'a ProjectConfig) -> Self {
        Self {
            source_roots,
            project_config,
        }
    }
}

impl<'a> FileProcessor<'a, ProjectFile<'a>> for ExternalDependencyExtractor<'a> {
    type ProcessedFile = FileModule<'a>;

    fn process(&self, file_path: ProjectFile<'a>) -> DiagnosticResult<Self::ProcessedFile> {
        // NOTE: check-external does not currently make use of the module tree,
        // but it is very likely to do so in the future.
        let module = Arc::new(ModuleNode::empty());
        let mut file_module = FileModule::new(file_path, module);
        let external_imports: Vec<Dependency> = get_normalized_imports(
            self.source_roots,
            file_module.file_path(),
            file_module.contents(),
            self.project_config.ignore_type_checking_imports,
            false,
        )?
        .into_iter()
        .filter_map(|import| {
            if !filesystem::is_project_import(self.source_roots, &import.module_path) {
                Some(Dependency::Import(import))
            } else {
                // Remove directives that match irrelevant imports
                file_module
                    .ignore_directives
                    .remove_matching_directives(file_module.line_number(import.import_offset));
                None
            }
        })
        .collect();
        file_module.extend_dependencies(external_imports);
        Ok(file_module)
    }
}
