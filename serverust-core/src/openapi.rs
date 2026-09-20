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
    security::{HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme},
};

/// Nome do esquema de segurança no documento.
///
/// É o rótulo que o botão "Authorize" do Scalar/Swagger UI exibe. Fixo de
/// propósito: hoje o framework só sabe descrever um esquema, e deixá-lo
/// configurável sugeriria que há mais de um a escolher.
pub(crate) const ESQUEMA_BEARER: &str = "bearerAuth";

/// Acumula paths/operations + schemas até o momento de gerar o documento final.
#[derive(Default)]
pub(crate) struct OpenApiState {
    title: Option<String>,
    version: Option<String>,
    /// Para preservar ordem de inserção e mergear múltiplos métodos no mesmo path.
    paths: Vec<(String, HttpMethod, Operation, SegurancaDaRota)>,
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
        seguranca: SegurancaDaRota,
    ) {
        self.paths.push((path.into(), method, op, seguranca));
    }

    pub(crate) fn register_schema<T: ToSchema + PartialSchema>(&mut self) {
        let name = T::name().into_owned();
        let schema = <T as PartialSchema>::schema();
        self.schemas.insert(name, schema);
    }

    /// Monta o documento.
    ///
    /// `auth_instalada` decide se o documento fala de segurança. Sem
    /// autenticação instalada, declarar um esquema seria descrever uma
    /// exigência que não existe — e um documento que mente sobre segurança é
    /// pior do que um documento omisso, porque alguém confia nele.
    pub(crate) fn build(&self, auth_instalada: bool) -> OpenApi {
        let info = InfoBuilder::new()
            .title(self.title.clone().unwrap_or_else(|| "serverust".into()))
            .version(self.version.clone().unwrap_or_else(|| "0.1.0".into()))
            .build();

        // Agrupa operations por path (suporta múltiplos métodos no mesmo path).
        let mut grouped: BTreeMap<String, Vec<(HttpMethod, Operation)>> = BTreeMap::new();
        for (path, method, op, seguranca) in &self.paths {
            let mut op = op.clone();
            if auth_instalada {
                op.security = Some(seguranca.requisitos());
            }
            grouped
                .entry(path.clone())
                .or_default()
                .push((method.clone(), op));
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

        let components: Option<Components> = if self.schemas.is_empty() && !auth_instalada {
            None
        } else {
            let mut cb = ComponentsBuilder::new();
            for (name, schema) in &self.schemas {
                cb = cb.schema(name, schema.clone());
            }
            if auth_instalada {
                cb = cb.security_scheme(ESQUEMA_BEARER, esquema_bearer());
            }
            Some(cb.build())
        };

        let mut builder = OpenApiBuilder::new()
            .info(info)
            .paths(paths)
            .components(components);

        if auth_instalada {
            // Exigência no nível do documento: o default do serviço é pedir
            // credencial, e cada operação só precisa aparecer no documento
            // quando diverge disso. Espelha o default deny do runtime — se o
            // documento listasse a exigência rota a rota, esquecer uma
            // produziria docs que dizem "aberta" sobre uma rota fechada.
            builder = builder.security(Some([SecurityRequirement::new::<_, [&str; 0], _>(
                ESQUEMA_BEARER,
                [],
            )]));
        }

        builder.build()
    }
}

/// O que o documento precisa dizer sobre a segurança de uma rota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegurancaDaRota {
    /// Rota marcada `#[public]`: responde sem credencial.
    Publica,
    /// Rota sob o default deny, sem exigência de escopo.
    ExigeIdentidade,
    /// Rota sob o default deny que também exige escopos (via `#[authorize]`).
    ExigeEscopos(&'static [&'static str]),
}

impl SegurancaDaRota {
    fn requisitos(self) -> Vec<SecurityRequirement> {
        match self {
            // `security: []` na operação é como o OpenAPI diz "esta aqui não
            // exige nada", sobrescrevendo o requisito global. Um vetor vazio,
            // e não a ausência do campo: ausência herdaria o global e faria o
            // documento afirmar que a rota pública é protegida.
            Self::Publica => Vec::new(),
            Self::ExigeIdentidade => vec![SecurityRequirement::new::<_, [&str; 0], _>(
                ESQUEMA_BEARER,
                [],
            )],
            Self::ExigeEscopos(escopos) => {
                vec![SecurityRequirement::new(
                    ESQUEMA_BEARER,
                    escopos.iter().copied(),
                )]
            }
        }
    }
}

/// O esquema que descreve `Authorization: Bearer <jwt>`.
fn esquema_bearer() -> SecurityScheme {
    SecurityScheme::Http(
        HttpBuilder::new()
            .scheme(HttpAuthScheme::Bearer)
            .bearer_format("JWT")
            .description(Some(
                "JWT emitido pelo provedor de identidade configurado. \
                 Envie em `Authorization: Bearer <token>`.",
            ))
            .build(),
    )
}

/// HTML embutido que carrega Scalar API Reference via CDN apontando para `spec_url`.
pub(crate) fn swagger_ui_html(spec_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>API Reference</title>
  </head>
  <body>
    <script
      id="api-reference"
      data-url="{spec_url}"
      data-configuration='{{"theme":"default","layout":"modern"}}'
    ></script>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
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
