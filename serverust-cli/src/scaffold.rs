//! Geração de scaffolding em disco.
//!
//! Todas as funções recebem o diretório base por argumento (em vez de
//! `current_dir()`) para permitirem testes em diretórios temporários sem
//! mutar o estado global do processo.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::GenerateKind;
use crate::templates;

/// Cria um novo projeto serverust em `base/<name>`.
pub fn new_project(base: &Path, name: &str) -> Result<()> {
    let root = base.join(name);
    if root.exists() {
        bail!("target directory '{}' already exists", root.display());
    }
    fs::create_dir_all(root.join("src/modules"))
        .with_context(|| format!("creating src/modules in {}", root.display()))?;
    fs::create_dir_all(root.join("src/shared"))
        .with_context(|| format!("creating src/shared in {}", root.display()))?;

    write_file(
        &root.join("Cargo.toml"),
        &templates::project_cargo_toml(name),
    )?;
    write_file(
        &root.join("serverust.toml"),
        &templates::project_serverust_toml(name),
    )?;
    write_file(&root.join("src/main.rs"), &templates::project_main_rs())?;
    write_file(
        &root.join("src/modules/mod.rs"),
        &templates::project_modules_mod_rs(),
    )?;
    write_file(
        &root.join("src/shared/mod.rs"),
        &templates::project_shared_mod_rs(),
    )?;
    Ok(())
}

/// Gera scaffolding para um artefato (`controller`, `service`, etc.) em
/// `base/src/...`.
pub fn generate(base: &Path, kind: GenerateKind, name: &str, crud: bool) -> Result<()> {
    match kind {
        GenerateKind::Resource => generate_resource(base, name),
        GenerateKind::Module => generate_module(base, name, crud, crud),
        GenerateKind::Controller => generate_single_module_file(
            base,
            name,
            format!("{name}.controller.rs"),
            templates::controller(name),
        ),
        GenerateKind::Service => generate_single_module_file(
            base,
            name,
            format!("{name}.service.rs"),
            templates::service(name),
        ),
        GenerateKind::Pipe => generate_shared(
            base,
            "pipes",
            format!("{name}.pipe.rs"),
            templates::pipe(name),
        ),
        GenerateKind::Guard => generate_shared(
            base,
            "guards",
            format!("{name}.guard.rs"),
            templates::guard(name),
        ),
        GenerateKind::Interceptor => generate_shared(
            base,
            "interceptors",
            format!("{name}.interceptor.rs"),
            templates::interceptor(name),
        ),
        GenerateKind::Filter => generate_shared(
            base,
            "filters",
            format!("{name}.filter.rs"),
            templates::filter(name),
        ),
    }
}

fn generate_module(base: &Path, name: &str, with_dto: bool, with_tests: bool) -> Result<()> {
    let dir = base.join("src/modules").join(name);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    write_file(
        &dir.join("mod.rs"),
        &if with_dto {
            templates::resource_mod_rs(name, with_tests)
        } else {
            templates::module_mod_rs(name, with_tests)
        },
    )?;
    write_file(
        &dir.join(format!("{name}.controller.rs")),
        &templates::controller(name),
    )?;
    write_file(
        &dir.join(format!("{name}.service.rs")),
        &templates::service(name),
    )?;
    if with_dto {
        write_file(&dir.join(format!("{name}.dto.rs")), &templates::dto(name))?;
    }
    if with_tests {
        write_file(
            &dir.join(format!("{name}.tests.rs")),
            &templates::module_test(name),
        )?;
    }
    Ok(())
}

fn generate_resource(base: &Path, name: &str) -> Result<()> {
    generate_module(base, name, true, false)
}

fn generate_single_module_file(
    base: &Path,
    name: &str,
    filename: String,
    body: String,
) -> Result<()> {
    let dir = base.join("src/modules").join(name);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    write_file(&dir.join(filename), &body)
}

fn generate_shared(base: &Path, kind_dir: &str, filename: String, body: String) -> Result<()> {
    let dir = base.join("src/shared").join(kind_dir);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    write_file(&dir.join(filename), &body)
}

fn write_file(path: &PathBuf, body: &str) -> Result<()> {
    fs::write(path, body).with_context(|| format!("writing {}", path.display()))
}
