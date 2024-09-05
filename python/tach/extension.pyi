from pathlib import Path
from typing import Literal

def get_project_imports(
    source_roots: list[str],
    file_path: str,
    ignore_type_checking_imports: bool,
) -> list[tuple[str, int]]: ...
def get_external_imports(
    source_roots: list[str],
    file_path: str,
    ignore_type_checking_imports: bool,
) -> list[tuple[str, int]]: ...
def get_normalized_imports(
    source_roots: list[str],
    file_path: str,
    ignore_type_checking_imports: bool,
) -> list[tuple[str, int]]: ...
def set_excluded_paths(
    project_root: str, exclude_paths: list[str], use_regex_matching: bool
) -> None: ...
def check_external_dependencies(
    project_root: str,
    source_roots: list[str],
    module_mappings: dict[str, list[str]],
    ignore_type_checking_imports: bool,
) -> dict[str, list[str]]: ...
def create_dependency_report(
    project_root: str,
    source_roots: list[str],
    path: str,
    include_dependency_modules: list[str] | None,
    include_usage_modules: list[str] | None,
    skip_dependencies: bool,
    skip_usages: bool,
    ignore_type_checking_imports: bool,
) -> str: ...
def create_computation_cache_key(
    project_root: str,
    source_roots: list[str],
    action: str,
    py_interpreter_version: str,
    file_dependencies: list[str],
    env_dependencies: list[str],
    backend: str,
) -> str: ...
def check_computation_cache(
    project_root: str, cache_key: str
) -> tuple[list[tuple[int, str]], int] | None: ...
def update_computation_cache(
    project_root: str, cache_key: str, value: tuple[list[tuple[int, str]], int]
) -> None: ...
def parse_project_config(filepath: str) -> ProjectConfig: ...
def parse_interface_members(source_roots: list[Path], path: str) -> list[str]: ...
def check(
    project_root: str,
    project_config_path: str,
    exclude_paths: list[str],
) -> CheckResult: ...

class ErrorInfo:
    def is_dependency_error(self) -> bool: ...
    def to_pystring(self) -> str: ...
    def is_deprecated(self) -> bool: ...

class BoundaryError:
    file_path: Path
    line_number: int
    import_mod_path: str
    error_info: ErrorInfo

class CheckResult:
    errors: list[BoundaryError]
    deprecated_warnings: list[BoundaryError]
    warnings: list[str]

class DependencyConfig:
    path: str
    deprecated: bool

class ModuleConfig:
    path: str
    depends_on: list[DependencyConfig]
    strict: bool

CacheBackend = Literal["disk"]

class CacheConfig:
    backend: CacheBackend
    file_dependencies: list[str]
    env_dependencies: list[str]

class ExternalDependencyConfig:
    exclude: list[str]

class ProjectConfig:
    modules: list[ModuleConfig]
    cache: CacheConfig
    external: ExternalDependencyConfig
    exclude: list[str]
    source_roots: list[str]
    exact: bool
    disable_logging: bool
    ignore_type_checking_imports: bool
    forbid_circular_dependencies: bool
    use_regex_matching: bool
