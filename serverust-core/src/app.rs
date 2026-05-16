use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::routing::get;
use tokio::net::{TcpListener, ToSocketAddrs};
use utoipa::{PartialSchema, ToSchema};

use crate::config::ServerustConfig;
use crate::container::Container;
use crate::events::{EventDispatcher, EventHandler, EventHandlerRegistry};
use crate::openapi::{OpenApiState, redoc_html, swagger_ui_html};
use crate::pipeline::Interceptor;
use crate::route::IntoRoute;

type RouterMutator = Box<dyn FnOnce(Router<Container>) -> Router<Container> + Send + Sync>;

/// Builder principal do framework.
///
/// Acumula rotas, services de DI, configuração de OpenAPI e middleware
/// (interceptors). Use [`run_http`](Self::run_http) para servir local, ou a
/// trait `AppRuntime` (do crate `serverust-lambda`) para o método `.run()`
/// que detecta automaticamente entre Lambda e HTTP local.
///
/// # Exemplo
///
/// ```no_run
/// use std::sync::Arc;
/// use serverust_core::App;
/// use serverust_macros::{get, injectable};
///
/// #[injectable]
/// struct Greeter;
///
/// impl Greeter {
///     fn hi(&self) -> String { "hello".into() }
/// }
///
/// #[get("/")]
/// async fn root(
///     axum::extract::State(g): axum::extract::State<Arc<Greeter>>,
/// ) -> String {
///     g.hi()
/// }
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     App::new()
///         .openapi_info("My API", "0.1.0")
///         .provide::<Greeter>(Arc::new(Greeter))
///         .route(root)
///         .run_http("127.0.0.1:3000")
///         .await
/// }
/// ```
///
/// # Rotas de documentação
///
/// [`into_router`](Self::into_router) injeta automaticamente três rotas:
/// `/openapi.json` (OpenAPI 3.1), `/docs` (Scalar API Reference) e `/redoc` (ReDoc).
/// Customize os paths via [`docs`](Self::docs) e [`redoc`](Self::redoc).
pub struct App {
    router: Router<Container>,
    container: Container,
    openapi: OpenApiState,
    openapi_path: &'static str,
    docs_path: &'static str,
    redoc_path: &'static str,
    interceptors: Vec<RouterMutator>,
    // TypeId::of::<EventHandlerRegistry<E>>() → Box<EventHandlerRegistry<E>>
    event_registries: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl App {
    /// Cria um App vazio com defaults: `/openapi.json`, `/docs`, `/redoc`.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            container: Container::new(),
            openapi: OpenApiState::default(),
            openapi_path: "/openapi.json",
            docs_path: "/docs",
            redoc_path: "/redoc",
            interceptors: Vec::new(),
            event_registries: HashMap::new(),
        }
    }

    /// Customiza `title` e `version` do documento OpenAPI gerado.
    pub fn openapi_info(mut self, title: impl Into<String>, version: impl Into<String>) -> Self {
        self.openapi.set_info(title, version);
        self
    }

    /// Registra um schema `T: ToSchema` em `components.schemas` do OpenAPI.
    pub fn register_schema<T: ToSchema + PartialSchema>(mut self) -> Self {
        self.openapi.register_schema::<T>();
        self
    }

    /// Customiza o path em que o Scalar API Reference é servido (default `/docs`).
    pub fn docs(mut self, path: &'static str) -> Self {
        self.docs_path = path;
        self
    }

    /// Customiza o path em que o ReDoc é servido (default `/redoc`).
    pub fn redoc(mut self, path: &'static str) -> Self {
        self.redoc_path = path;
        self
    }

    /// Registra um service com lifetime Singleton no container.
    ///
    /// `T` pode ser `dyn Trait`: `app.provide::<dyn MyService>(Arc::new(impl))`.
    /// Handlers extraem o serviço via `State<Arc<dyn MyService>>`.
    pub fn provide<T: ?Sized + Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.container.insert(value);
        self
    }

    /// API de teste: substitui o provider de `T` por uma instância mock.
    /// `override` é palavra reservada — chame como `app.r#override::<...>(...)`.
    pub fn r#override<T: ?Sized + Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.container.insert(value);
        self
    }

    /// Registra um interceptor (tower middleware) sobre as rotas do usuário.
    ///
    /// Aplicado em [`Self::into_router`] apenas às rotas registradas via
    /// [`Self::route`] — as rotas de documentação (`/openapi.json`, `/docs`,
    /// `/redoc`) ficam de fora intencionalmente, para que não dependam da
    /// pipeline de negócio (ex.: autenticação, rate limiting).
    pub fn interceptor<I: Interceptor>(mut self, interceptor: I) -> Self {
        let interceptor = std::sync::Arc::new(interceptor);
        let mutator: RouterMutator = Box::new(move |router: Router<Container>| {
            let interceptor = interceptor.clone();
            let layer = axum::middleware::from_fn(move |req: Request, next: Next| {
                let interceptor = interceptor.clone();
                async move { interceptor.intercept(req, next).await }
            });
            router.layer(layer)
        });
        self.interceptors.push(mutator);
        self
    }

    /// Injeta uma [`ServerustConfig`] tipada no container. Handlers podem extraí-la via
    /// `State<Arc<ServerustConfig>>`.
    pub fn config(self, cfg: ServerustConfig) -> Self {
        self.provide::<ServerustConfig>(Arc::new(cfg))
    }

    /// Registra um handler anotado por `#[get]`, `#[post]`, etc.
    pub fn route<R: IntoRoute>(mut self, handler: R) -> Self {
        let route = handler.into_route();
        self.openapi
            .push_operation(route.path, route.method, route.operation);
        self.router = self.router.route(route.path, route.method_router);
        self
    }

    /// Registra um [`EventHandler<E>`] tipado.
    ///
    /// Múltiplos handlers para o mesmo tipo `E` são acumulados e executados em
    /// sequência por [`EventDispatcher::dispatch_event`]. O [`Container`] é
    /// compartilhado entre handlers HTTP e event handlers — os mesmos serviços
    /// registrados via [`Self::provide`] ficam disponíveis em `ctx`.
    pub fn event<E, H>(mut self, handler: H) -> Self
    where
        E: Send + 'static,
        H: EventHandler<E>,
    {
        let key = TypeId::of::<EventHandlerRegistry<E>>();
        let registry = self
            .event_registries
            .entry(key)
            .or_insert_with(|| Box::new(EventHandlerRegistry::<E>::new()));
        registry
            .downcast_mut::<EventHandlerRegistry<E>>()
            .expect("type invariant: key matches registry type")
            .register(handler);
        self
    }

    /// Constrói um [`EventDispatcher<E>`] com todos os handlers registrados
    /// para o tipo `E` e uma cópia do [`Container`] compartilhado.
    pub fn into_event_dispatcher<E: Send + 'static>(mut self) -> EventDispatcher<E> {
        let key = TypeId::of::<EventHandlerRegistry<E>>();
        let registry = self
            .event_registries
            .remove(&key)
            .and_then(|b| b.downcast::<EventHandlerRegistry<E>>().ok())
            .unwrap_or_else(|| Box::new(EventHandlerRegistry::<E>::new()));
        registry.into_dispatcher(self.container)
    }

    /// Constrói o `axum::Router` final adicionando `/openapi.json`, `/docs` e `/redoc`.
    pub fn into_router(self) -> Router {
        let doc = self.openapi.build();
        let json = doc.to_json().unwrap_or_else(|_| "{}".to_string());
        let swagger_html = swagger_ui_html(self.openapi_path);
        let redoc_page = redoc_html(self.openapi_path);

        // Aplica interceptors sobre as rotas do usuário antes de juntar com as
        // rotas de documentação — isto garante que /openapi.json, /docs e
        // /redoc NÃO sejam envolvidos pela pipeline de middleware do usuário.
        let mut user_router = self.router;
        for mutator in self.interceptors {
            user_router = mutator(user_router);
        }

        user_router
            .route(
                self.openapi_path,
                get(move || {
                    let json = json.clone();
                    async move {
                        (
                            [(header::CONTENT_TYPE, "application/json")],
                            json,
                        )
                            .into_response()
                    }
                }),
            )
            .route(
                self.docs_path,
                get(move || {
                    let html = swagger_html.clone();
                    async move {
                        (
                            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                            html,
                        )
                            .into_response()
                    }
                }),
            )
            .route(
                self.redoc_path,
                get(move || {
                    let html = redoc_page.clone();
                    async move {
                        (
                            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                            html,
                        )
                            .into_response()
                    }
                }),
            )
            .with_state(self.container)
    }

    /// Sobe um servidor HTTP local ligado em `addr` (ex.: `"127.0.0.1:3000"`).
    ///
    /// Imprime no stderr o endereço efetivo + URLs de documentação assim que o
    /// listener fica pronto, para o desenvolvedor saber onde conectar:
    ///
    /// ```text
    ///   🦀 serverust on http://0.0.0.0:3000
    ///      docs:    http://0.0.0.0:3000/docs
    ///      openapi: http://0.0.0.0:3000/openapi.json
    /// ```
    pub async fn run_http<A: ToSocketAddrs>(self, addr: A) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let docs_path = self.docs_path;
        let openapi_path = self.openapi_path;
        let router = self.into_router();
        eprintln!();
        eprintln!("  🦀 serverust on http://{local}");
        eprintln!("     docs:    http://{local}{docs_path}");
        eprintln!("     openapi: http://{local}{openapi_path}");
        eprintln!();
        axum::serve(listener, router).await
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
