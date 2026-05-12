//! Geração de OpenAPI 3.1 a partir das rotas registradas no [`crate::App`].
//!
//! O App acumula uma lista de rotas (path + método + `Operation`) e schemas
//! registrados pelo usuário via `App::register_schema::<T>()`. A partir disso,
//! constrói dinamicamente um `utoipa::openapi::OpenApi` que é serializado em
//! `/openapi.json`. `/docs` e `/redoc` carregam o JSON via HTML/CDN.

use std::collections::BTreeMap;

use utoipa::PartialSchema;
use utoipa::ToSchema;
use utoipa::openapi::{
    HttpMethod, InfoBuilder, OpenApi, OpenApiBuilder, PathItem, Paths, PathsBuilder, RefOr, Schema,
    path::{Operation, PathItemBuilder},
    schema::{Components, ComponentsBuilder},
};

/// Acumula paths/operations + schemas até o momento de gerar o documento final.
#[derive(Default)]
pub(crate) struct OpenApiState {
    title: Option<String>,
    version: Option<String>,
    /// Para preservar ordem de inserção e mergear múltiplos métodos no mesmo path.
    paths: Vec<(String, HttpMethod, Operation)>,
    schemas: BTreeMap<String, RefOr<Schema>>,
}

impl OpenApiState {
    pub(crate) fn set_info(&mut self, title: impl Into<String>, version: impl Into<String>) {
        self.title = Some(title.into());
        self.version = Some(version.into());
    }

    pub(crate) fn push_operation(
        &mut self,
        path: impl Into<String>,
        method: HttpMethod,
        op: Operation,
    ) {
        self.paths.push((path.into(), method, op));
    }

    pub(crate) fn register_schema<T: ToSchema + PartialSchema>(&mut self) {
        let name = T::name().into_owned();
        let schema = <T as PartialSchema>::schema();
        self.schemas.insert(name, schema);
    }

    pub(crate) fn build(&self) -> OpenApi {
        let info = InfoBuilder::new()
            .title(self.title.clone().unwrap_or_else(|| "serverust".into()))
            .version(self.version.clone().unwrap_or_else(|| "0.1.0".into()))
            .build();

        // Agrupa operations por path (suporta múltiplos métodos no mesmo path).
        let mut grouped: BTreeMap<String, Vec<(HttpMethod, Operation)>> = BTreeMap::new();
        for (path, method, op) in &self.paths {
            grouped
                .entry(path.clone())
                .or_default()
                .push((method.clone(), op.clone()));
        }

        let mut paths_builder = PathsBuilder::new();
        for (path, ops) in grouped {
            let mut item_builder = PathItemBuilder::new();
            for (method, op) in ops {
                item_builder = item_builder.operation(method, op);
            }
            let item: PathItem = item_builder.build();
            paths_builder = paths_builder.path(path, item);
        }
        let paths: Paths = paths_builder.build();

        let components: Option<Components> = if self.schemas.is_empty() {
            None
        } else {
            let mut cb = ComponentsBuilder::new();
            for (name, schema) in &self.schemas {
                cb = cb.schema(name, schema.clone());
            }
            Some(cb.build())
        };

        OpenApiBuilder::new()
            .info(info)
            .paths(paths)
            .components(components)
            .build()
    }
}

/// HTML embutido que carrega Swagger UI via CDN apontando para `spec_url`.
pub(crate) fn swagger_ui_html(spec_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>API Docs</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css" />
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script>
      window.onload = function () {{
        window.ui = SwaggerUIBundle({{
          url: "{spec_url}",
          dom_id: "#swagger-ui",
        }});
      }};
    </script>
  </body>
</html>"##
    )
}

/// HTML embutido que carrega ReDoc via CDN apontando para `spec_url`.
pub(crate) fn redoc_html(spec_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>API ReDoc</title>
  </head>
  <body>
    <redoc spec-url="{spec_url}"></redoc>
    <script src="https://cdn.jsdelivr.net/npm/redoc@next/bundles/redoc.standalone.js"></script>
  </body>
</html>"##
    )
}
