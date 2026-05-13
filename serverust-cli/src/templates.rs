//! Templates de texto para o scaffolding.
//!
//! Cada função devolve uma `String` pronta para ser escrita em disco. Os
//! templates são intencionalmente simples (sem engine externo) para manter o
//! binário enxuto e fácil de manter — substituições são feitas via `replace`.

const NAME_PLACEHOLDER: &str = "{{NAME}}";
const TYPE_PLACEHOLDER: &str = "{{TYPE}}";

pub fn project_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
serverust-core   = "0.1"
serverust-lambda = "0.1"
serverust-macros = "0.1"
tokio  = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
serde  = {{ version = "1", features = ["derive"] }}
utoipa = {{ version = "5", features = ["macros"] }}
"#
    )
}

pub fn project_serverust_toml(name: &str) -> String {
    format!(
        r#"# serverust.toml — configuração do projeto "{name}"
# Perfis: default, dev, staging, prod
# Selecione com SERVERUST_PROFILE=prod ou ServerustConfig::load_for_profile("prod")
# Override por env: SERVERUST_SERVER__PORT=8080

[default.server]
host = "127.0.0.1"
port = 3000

[default.lambda]
memory_size = 128
timeout_seconds = 30

[default.telemetry]
log_level = "info"
format = "json"

[default.openapi]
title = "{name}"
version = "0.1.0"
docs_path = "/docs"
redoc_path = "/redoc"

[dev.server]
port = 3001

[prod.server]
host = "0.0.0.0"
port = 8080
"#
    )
}

pub fn project_main_rs() -> String {
    r#"use serde::Serialize;
use serverust_core::{App, extract::Json};
use serverust_lambda::AppRuntime;
use serverust_macros::get;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
struct HelloResponse {
    message: String,
}

#[get("/", response = HelloResponse)]
async fn hello() -> Json<HelloResponse> {
    Json(HelloResponse {
        message: "Hello, serverust!".into(),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Local: HTTP em 0.0.0.0:3000. Lambda: detecta automaticamente.
    // Acesse http://localhost:3000/docs para a referência interativa Scalar.
    App::new()
        .openapi_info("My API", "0.1.0")
        .register_schema::<HelloResponse>()
        .route(hello)
        .run()
        .await?;
    Ok(())
}
"#
    .to_string()
}

pub fn project_modules_mod_rs() -> String {
    "// declare seus módulos aqui: `pub mod users;`\n".to_string()
}

pub fn project_shared_mod_rs() -> String {
    "// componentes compartilhados: guards, pipes, interceptors, filters\n".to_string()
}

pub fn controller(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use serverust_core::extract::Path;
use serverust_macros::get;

#[get("/{{NAME}}/{id}")]
pub async fn show_{{NAME}}(Path(id): Path<u64>) -> String {
    format!("{{TYPE}}::show id={id}")
}
"#,
        name,
        &type_name,
    )
}

pub fn service(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use serverust_macros::injectable;

#[injectable]
pub struct {{TYPE}}Service;

impl {{TYPE}}Service {
    pub fn new() -> Self {
        Self
    }
}

impl Default for {{TYPE}}Service {
    fn default() -> Self {
        Self::new()
    }
}
"#,
        name,
        &type_name,
    )
}

pub fn dto(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Create{{TYPE}}Dto {
    #[validate(length(min = 1))]
    pub name: String,
}
"#,
        name,
        &type_name,
    )
}

pub fn module_mod_rs(name: &str, with_tests: bool) -> String {
    let mut out = format!(
        "#[path = \"{name}.controller.rs\"]\npub mod controller;\n\
         #[path = \"{name}.service.rs\"]\npub mod service;\n"
    );
    if with_tests {
        out.push_str(&format!(
            "#[cfg(test)]\n#[path = \"{name}.tests.rs\"]\nmod tests;\n"
        ));
    }
    out
}

pub fn resource_mod_rs(name: &str, with_tests: bool) -> String {
    format!(
        "{base}#[path = \"{name}.dto.rs\"]\npub mod dto;\n",
        base = module_mod_rs(name, with_tests),
    )
}

pub fn module_test(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"#[test]
fn {{NAME}}_crud_module_smoke() {
    let _service = super::service::{{TYPE}}Service::new();
    assert!(super::controller::show_{{NAME}} as usize > 0);
}
"#,
        name,
        &type_name,
    )
}

pub fn pipe(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use serverust_core::pipeline::Pipe;

pub struct {{TYPE}}Pipe;

impl Pipe<String> for {{TYPE}}Pipe {
    type Output = String;

    fn transform(input: String) -> Result<Self::Output, axum::response::Response> {
        Ok(input)
    }
}
"#,
        name,
        &type_name,
    )
}

pub fn guard(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use axum::http::request::Parts;
use axum::response::Response;
use serverust_core::pipeline::Guard;

pub struct {{TYPE}}Guard;

impl Guard for {{TYPE}}Guard {
    async fn check(_parts: &Parts) -> Result<(), Response> {
        Ok(())
    }
}
"#,
        name,
        &type_name,
    )
}

pub fn interceptor(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use serverust_core::pipeline::Interceptor;

pub struct {{TYPE}}Interceptor;

impl Interceptor for {{TYPE}}Interceptor {
    async fn intercept(&self, req: Request, next: Next) -> Response {
        next.run(req).await
    }
}
"#,
        name,
        &type_name,
    )
}

pub fn filter(name: &str) -> String {
    let type_name = pascal_case(name);
    template_with_name_type(
        r#"use axum::response::{IntoResponse, Response};

/// Filter (mapeador de erros) inspirado em ExceptionFilter do NestJS.
pub struct {{TYPE}}Filter;

impl {{TYPE}}Filter {
    pub fn handle<E: std::error::Error>(err: E) -> Response {
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
    }
}
"#,
        name,
        &type_name,
    )
}

fn template_with_name_type(template: &str, name: &str, type_name: &str) -> String {
    template
        .replace(NAME_PLACEHOLDER, name)
        .replace(TYPE_PLACEHOLDER, type_name)
}

fn pascal_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = true;
    for ch in s.chars() {
        if ch == '_' || ch == '-' {
            up = true;
            continue;
        }
        if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}
