use serverust_cli::cli::GenerateKind;
use serverust_cli::scaffold;
use tempfile::tempdir;

#[test]
fn new_project_creates_expected_layout() {
    let tmp = tempdir().unwrap();
    scaffold::new_project(tmp.path(), "myapp").expect("scaffold new");

    let root = tmp.path().join("myapp");
    assert!(root.join("Cargo.toml").is_file(), "Cargo.toml");
    assert!(root.join("serverust.toml").is_file(), "serverust.toml");
    assert!(root.join("src").is_dir(), "src/");
    assert!(root.join("src/main.rs").is_file(), "src/main.rs");
    assert!(root.join("src/modules").is_dir(), "src/modules");
    assert!(root.join("src/shared").is_dir(), "src/shared");

    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("name = \"myapp\""), "cargo name");
    assert!(
        cargo.contains("serverust-core"),
        "depends on serverust-core"
    );

    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(main.contains("App::new"), "main uses App::new");
}

#[test]
fn new_project_refuses_if_dir_exists() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("myapp");
    std::fs::create_dir_all(&target).unwrap();
    let err = scaffold::new_project(tmp.path(), "myapp").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("already exists") || msg.contains("myapp"),
        "error mentions target: {msg}"
    );
}

#[test]
fn generate_controller_creates_file() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/modules")).unwrap();
    scaffold::generate(tmp.path(), GenerateKind::Controller, "users", false).expect("generate");

    let file = tmp.path().join("src/modules/users/users.controller.rs");
    assert!(file.is_file(), "controller file at {file:?}");
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("users"), "mentions name");
    assert!(content.contains("#[get"), "uses #[get] route macro");
}

#[test]
fn generate_service_creates_file() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/modules")).unwrap();
    scaffold::generate(tmp.path(), GenerateKind::Service, "users", false).expect("generate");
    let file = tmp.path().join("src/modules/users/users.service.rs");
    assert!(file.is_file(), "service file");
    let c = std::fs::read_to_string(&file).unwrap();
    assert!(c.contains("UsersService"), "type name");
}

#[test]
fn generate_module_creates_three_files() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/modules")).unwrap();
    scaffold::generate(tmp.path(), GenerateKind::Module, "orders", false).expect("generate");
    let dir = tmp.path().join("src/modules/orders");
    assert!(dir.join("mod.rs").is_file(), "mod.rs");
    assert!(dir.join("orders.controller.rs").is_file(), "controller");
    assert!(dir.join("orders.service.rs").is_file(), "service");
}

#[test]
fn generate_resource_includes_dto_and_module() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/modules")).unwrap();
    scaffold::generate(tmp.path(), GenerateKind::Resource, "products", false).expect("generate");
    let dir = tmp.path().join("src/modules/products");
    assert!(dir.join("mod.rs").is_file());
    assert!(dir.join("products.controller.rs").is_file());
    assert!(dir.join("products.service.rs").is_file());
    assert!(dir.join("products.dto.rs").is_file());
}

#[test]
fn generate_pipe_guard_interceptor_filter_create_files() {
    let kinds = [
        (
            GenerateKind::Pipe,
            "validate",
            "shared/pipes/validate.pipe.rs",
        ),
        (GenerateKind::Guard, "auth", "shared/guards/auth.guard.rs"),
        (
            GenerateKind::Interceptor,
            "log",
            "shared/interceptors/log.interceptor.rs",
        ),
        (
            GenerateKind::Filter,
            "http",
            "shared/filters/http.filter.rs",
        ),
    ];
    for (kind, name, rel) in kinds {
        let tmp = tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        scaffold::generate(tmp.path(), kind, name, false).expect("generate");
        let file = tmp.path().join("src").join(rel);
        assert!(file.is_file(), "expected {file:?}");
    }
}

#[test]
fn generate_module_with_crud_creates_dto_and_tests() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/modules")).unwrap();
    scaffold::generate(tmp.path(), GenerateKind::Module, "users", true).expect("generate");
    let dir = tmp.path().join("src/modules/users");
    assert!(dir.join("users.dto.rs").is_file());
    assert!(dir.join("users.tests.rs").is_file());
    let mod_rs = std::fs::read_to_string(dir.join("mod.rs")).unwrap();
    assert!(mod_rs.contains("mod tests"));
}
