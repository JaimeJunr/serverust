//! Macros declarativas do framework **serverust**.
//!
//! # Macros de rota
//!
//! `#[get("/path")]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` transformam
//! a função anotada em uma struct unit com o mesmo nome, implementando
//! `serverust_core::IntoRoute`. A struct é registrada via
//! `App::route(handler)`. A macro emite também uma `utoipa::openapi::Operation`
//! com `operation_id = nome da fn`, alimentando o `/openapi.json` automático.
//!
//! ```ignore
//! use serverust_macros::{get, post};
//!
//! #[get("/health")]
//! async fn health() -> &'static str { "ok" }
//!
//! #[post("/users")]
//! async fn create_user() -> &'static str { "created" }
//! ```
//!
//! # Derive macros
//!
//! - `#[derive(ApiError)]` — emite `impl ApiError + IntoResponse` lendo
//!   `#[status(N)]` e `#[message("...")]` por variante. Use com handlers
//!   `Result<T, MyError>` e converta erro em HTTP automaticamente via `?`.
//!
//! ```ignore
//! use serverust_macros::ApiError;
//!
//! #[derive(Debug, ApiError)]
//! enum UserError {
//!     #[status(404)] #[message("user not found")] NotFound,
//!     #[status(409)] #[message("email exists")]   EmailExists,
//! }
//! ```
//!
//! # Atributos de pipeline
//!
//! - `#[guard(MyGuard)]` — coloca **acima** de `#[get/post/...]`; injeta
//!   `GuardCheck<MyGuard>` no início da assinatura. Múltiplos `#[guard]` são
//!   empilháveis.
//! - `#[injectable]` — marker em struct/enum para sinalizar que o tipo é uma
//!   dependência. Aceita `#[injectable(static)]` como hint de static dispatch.
//!   O registro real é feito via `App::provide`.
//! - `#[metric(name = "...", unit = "Milliseconds")]` — cronometra a função
//!   (sync ou async) e emite uma métrica EMF via `serverust_telemetry::emit_emf`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    Data, DeriveInput, Expr, ExprLit, Fields, FnArg, GenericArgument, Ident, Item, ItemFn, Lit,
    LitInt, LitStr, Meta, PathArguments, PathSegment, Token, Type, parse_macro_input, parse_quote,
    punctuated::Punctuated, spanned::Spanned,
};

/// Atributo da macro de rota: caminho obrigatório + `response = Type` opcional.
///
/// Exemplos:
/// - `#[get("/")]`
/// - `#[get("/users", response = UserResponse)]`
struct RouteAttr {
    path: LitStr,
    response_ty: Option<Type>,
    tag: Option<LitStr>,
    operation_id: Option<LitStr>,
    request_example: Option<LitStr>,
    response_example: Option<LitStr>,
}

impl Parse for RouteAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        let mut response_ty = None;
        let mut tag = None;
        let mut operation_id = None;
        let mut request_example = None;
        let mut response_example = None;

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "response" => response_ty = Some(input.parse::<Type>()?),
                "tag" => tag = Some(input.parse::<LitStr>()?),
                "operation_id" => operation_id = Some(input.parse::<LitStr>()?),
                "request_example" => request_example = Some(input.parse::<LitStr>()?),
                "response_example" => response_example = Some(input.parse::<LitStr>()?),
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "atributo inválido. Use: response, tag, operation_id, request_example, response_example",
                    ));
                }
            }
        }
        Ok(RouteAttr {
            path,
            response_ty,
            tag,
            operation_id,
            request_example,
            response_example,
        })
    }
}

/// Extrai o último ident de um caminho de tipo (`Foo`, `module::Foo`, `Vec<Foo>`).
fn type_ident_str(ty: &Type) -> String {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return seg.ident.to_string();
        }
    }
    String::new()
}

fn make_route(method: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    let route_attr = parse_macro_input!(attr as RouteAttr);
    let func = parse_macro_input!(item as ItemFn);

    let vis = func.vis.clone();
    let fn_name = func.sig.ident.clone();
    let path = route_attr.path;
    let tag = route_attr.tag;
    let request_example = route_attr.request_example;
    let response_example = route_attr.response_example;
    let operation_id = route_attr
        .operation_id
        .unwrap_or_else(|| LitStr::new(&fn_name.to_string(), Span::call_site()));
    let fn_name_str = fn_name.to_string();
    let method_ident = Ident::new(method, Span::call_site());
    let http_method_variant = Ident::new(
        match method {
            "get" => "Get",
            "post" => "Post",
            "put" => "Put",
            "patch" => "Patch",
            "delete" => "Delete",
            _ => "Get",
        },
        Span::call_site(),
    );

    let response_description = if let Some(example) = response_example {
        quote! { format!("OK. Example: {}", #example) }
    } else {
        quote! { "OK".to_string() }
    };

    let response_expr = if let Some(ty) = &route_attr.response_ty {
        let schema_name = type_ident_str(ty);
        quote! {
            ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new()
                .description(#response_description)
                .content(
                    "application/json",
                    ::serverust_core::__private::utoipa::openapi::ContentBuilder::new()
                        .schema(Some(
                            ::serverust_core::__private::utoipa::openapi::schema::Ref::from_schema_name(
                                #schema_name
                            )
                        ))
                        .build(),
                )
                .build()
        }
    } else {
        quote! {
            ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new()
                .description(#response_description)
                .build()
        }
    };

    let description_expr = if let Some(example) = request_example {
        quote! { Some(format!("Request example: {}", #example)) }
    } else {
        quote! { None::<String> }
    };

    let tag_expr = if let Some(tag) = tag {
        quote! { .tag(#tag) }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #vis struct #fn_name;

        impl ::serverust_core::IntoRoute for #fn_name {
            fn into_route(self) -> ::serverust_core::Route {
                #func

                let operation = ::serverust_core::__private::utoipa::openapi::path::OperationBuilder::new()
                    .operation_id(Some(#operation_id))
                    .summary(Some(#fn_name_str))
                    .description(#description_expr)
                    #tag_expr
                    .response("200", #response_expr)
                    .response("401", ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new().description("Unauthorized").build())
                    .response("403", ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new().description("Forbidden").build())
                    .response("422", ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new().description("Validation Error").build())
                    .response("500", ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new().description("Internal Server Error").build())
                    .build();

                ::serverust_core::Route::new(
                    #path,
                    ::serverust_core::__private::utoipa::openapi::HttpMethod::#http_method_variant,
                    ::serverust_core::__private::axum::routing::#method_ident(#fn_name),
                    operation,
                )
            }
        }
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    make_route("get", attr, item)
}

#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    make_route("post", attr, item)
}

#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    make_route("put", attr, item)
}

#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    make_route("patch", attr, item)
}

#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    make_route("delete", attr, item)
}

/// Aplica um `Guard` antes do handler.
///
/// Uso: posicione `#[guard(MyGuard)]` ACIMA da macro de rota
/// (`#[get(...)]`, `#[post(...)]`, etc.). Pode ser empilhada para
/// compor múltiplos guards. A macro reescreve a função adicionando
/// um extractor `GuardCheck<MyGuard>` no início da assinatura — o
/// axum executa o check antes do handler e curto-circuita com a
/// resposta de erro retornada pelo guard caso ele rejeite.
#[proc_macro_attribute]
pub fn guard(attr: TokenStream, item: TokenStream) -> TokenStream {
    let guard_ty = parse_macro_input!(attr as Type);
    let mut func = parse_macro_input!(item as ItemFn);

    // Garante nome único entre múltiplos `#[guard(...)]` empilhados,
    // varrendo os parâmetros existentes.
    let mut idx = 0usize;
    let unique_name = loop {
        let candidate = format!("__serverust_guard_check_{idx}");
        let collides = func.sig.inputs.iter().any(|arg| {
            if let syn::FnArg::Typed(pt) = arg {
                if let syn::Pat::Ident(pi) = &*pt.pat {
                    return pi.ident == candidate;
                }
            }
            false
        });
        if !collides {
            break candidate;
        }
        idx += 1;
    };

    let param_ident = Ident::new(&unique_name, Span::call_site());
    let new_param: syn::FnArg = parse_quote! {
        #param_ident: ::serverust_core::GuardCheck<#guard_ty>
    };
    // Insere no início para que rodar como FromRequestParts antes de
    // qualquer extractor que consuma o body (FromRequest).
    func.sig.inputs.insert(0, new_param);

    quote! { #func }.into()
}

/// Marca um tipo como injetável no container DI do `App`.
///
/// Aceita `#[injectable]` (default — Singleton via `Arc<dyn Trait>`) ou
/// `#[injectable(static)]` (opt-in, dispatch estático via generics). Em
/// ambos os casos o tipo original é re-emitido inalterado e ganha
/// `impl serverust_core::Injectable` como marker.
#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let attr2: proc_macro2::TokenStream = attr.into();
        let tokens: Vec<_> = attr2.into_iter().collect();
        let is_static = tokens.len() == 1
            && matches!(&tokens[0], proc_macro2::TokenTree::Ident(i) if i == "static");
        if !is_static {
            return syn::Error::new(
                Span::call_site(),
                "atributo inválido em #[injectable]: use `#[injectable]` ou `#[injectable(static)]`",
            )
            .to_compile_error()
            .into();
        }
    }

    let item_parsed = parse_macro_input!(item as Item);
    let ident = match &item_parsed {
        Item::Struct(s) => s.ident.clone(),
        Item::Enum(e) => e.ident.clone(),
        Item::Impl(im) => {
            return syn::Error::new(
                im.span(),
                "#[injectable] em impl block ainda não é suportado; aplique no struct/enum",
            )
            .to_compile_error()
            .into();
        }
        other => {
            return syn::Error::new(
                other.span(),
                "#[injectable] só pode ser aplicado em struct ou enum",
            )
            .to_compile_error()
            .into();
        }
    };

    let expanded = quote! {
        #item_parsed

        impl ::serverust_core::Injectable for #ident {}
    };

    expanded.into()
}

/// Emite uma métrica EMF (Embedded Metric Format) ao redor da função.
///
/// Uso: `#[metric(name = "ProcessingTime", unit = "Milliseconds")]`.
/// Suporta também `namespace = "..."` (default `"serverust"`).
///
/// A macro cronometra a execução da função (sync ou async) e emite o
/// payload EMF em stdout via `serverust_telemetry::emit_emf`. O valor da
/// métrica é o tempo decorrido em milissegundos (compatível com a unidade
/// recomendada). Useful para instrumentar handlers e serviços sem ruído.
#[proc_macro_attribute]
pub fn metric(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2: proc_macro2::TokenStream = attr.into();
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let metas = match syn::parse::Parser::parse2(parser, attr2) {
        Ok(m) => m,
        Err(err) => return err.to_compile_error().into(),
    };

    let mut name: Option<String> = None;
    let mut unit: Option<String> = None;
    let mut namespace: String = "serverust".to_string();

    for meta in metas {
        let Meta::NameValue(nv) = meta else {
            return syn::Error::new(meta.span(), "esperado `key = \"value\"`")
                .to_compile_error()
                .into();
        };
        let Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) = &nv.value
        else {
            return syn::Error::new(nv.value.span(), "esperado literal string")
                .to_compile_error()
                .into();
        };
        let key = nv
            .path
            .get_ident()
            .map(|i| i.to_string())
            .unwrap_or_default();
        match key.as_str() {
            "name" => name = Some(s.value()),
            "unit" => unit = Some(s.value()),
            "namespace" => namespace = s.value(),
            other => {
                return syn::Error::new(
                    nv.path.span(),
                    format!("chave desconhecida `{other}` em #[metric]"),
                )
                .to_compile_error()
                .into();
            }
        }
    }

    let Some(name) = name else {
        return syn::Error::new(Span::call_site(), "#[metric] requer `name = \"...\"`")
            .to_compile_error()
            .into();
    };
    let Some(unit) = unit else {
        return syn::Error::new(Span::call_site(), "#[metric] requer `unit = \"...\"`")
            .to_compile_error()
            .into();
    };

    let mut func = parse_macro_input!(item as ItemFn);
    let block = func.block.clone();
    let is_async = func.sig.asyncness.is_some();

    let new_block: syn::Block = if is_async {
        parse_quote! {{
            let __serverust_metric_start = ::std::time::Instant::now();
            let __serverust_metric_result = async #block.await;
            let __serverust_metric_elapsed = __serverust_metric_start.elapsed().as_millis() as f64;
            ::serverust_telemetry::emit_emf(#namespace, #name, #unit, __serverust_metric_elapsed);
            __serverust_metric_result
        }}
    } else {
        parse_quote! {{
            let __serverust_metric_start = ::std::time::Instant::now();
            let __serverust_metric_result = (move || #block)();
            let __serverust_metric_elapsed = __serverust_metric_start.elapsed().as_millis() as f64;
            ::serverust_telemetry::emit_emf(#namespace, #name, #unit, __serverust_metric_elapsed);
            __serverust_metric_result
        }}
    };
    *func.block = new_block;

    quote! { #func }.into()
}

/// Deriva `serverust_core::ApiError` + `axum::response::IntoResponse` para um enum.
///
/// Cada variante deve declarar `#[status(N)]` e `#[message("...")]`. A
/// resposta gerada é JSON `{ "error": "<message>" }` com o status HTTP indicado.
#[proc_macro_derive(ApiError, attributes(status, message))]
pub fn derive_api_error(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident.clone();

    let Data::Enum(data_enum) = input.data else {
        return syn::Error::new(input.span(), "ApiError só pode ser derivado em enums")
            .to_compile_error()
            .into();
    };

    let mut status_arms = Vec::new();
    let mut message_arms = Vec::new();

    for variant in &data_enum.variants {
        let v_ident = &variant.ident;

        let mut status_value: Option<LitInt> = None;
        let mut message_value: Option<LitStr> = None;

        for attr in &variant.attrs {
            if attr.path().is_ident("status") {
                match attr.parse_args::<LitInt>() {
                    Ok(lit) => status_value = Some(lit),
                    Err(err) => return err.to_compile_error().into(),
                }
            } else if attr.path().is_ident("message") {
                match attr.parse_args::<LitStr>() {
                    Ok(lit) => message_value = Some(lit),
                    Err(err) => return err.to_compile_error().into(),
                }
            }
        }

        let Some(status_lit) = status_value else {
            return syn::Error::new(
                v_ident.span(),
                format!("variante `{v_ident}` precisa de #[status(N)]"),
            )
            .to_compile_error()
            .into();
        };
        let Some(message_lit) = message_value else {
            return syn::Error::new(
                v_ident.span(),
                format!("variante `{v_ident}` precisa de #[message(\"...\")]"),
            )
            .to_compile_error()
            .into();
        };

        let pattern = match &variant.fields {
            Fields::Unit => quote! { Self::#v_ident },
            Fields::Unnamed(_) => quote! { Self::#v_ident(..) },
            Fields::Named(_) => quote! { Self::#v_ident { .. } },
        };

        status_arms.push(quote! { #pattern => #status_lit });
        message_arms.push(quote! { #pattern => #message_lit.to_string() });
    }

    let expanded = quote! {
        impl ::serverust_core::ApiError for #name {
            fn status(&self) -> u16 {
                match self {
                    #( #status_arms, )*
                }
            }

            fn message(&self) -> String {
                match self {
                    #( #message_arms, )*
                }
            }
        }

        impl ::serverust_core::__private::axum::response::IntoResponse for #name {
            fn into_response(self) -> ::serverust_core::__private::axum::response::Response {
                let status = ::serverust_core::__private::http::StatusCode::from_u16(
                    <Self as ::serverust_core::ApiError>::status(&self),
                )
                .unwrap_or(::serverust_core::__private::http::StatusCode::INTERNAL_SERVER_ERROR);
                let body = ::serverust_core::__private::serde_json::json!({
                    "error": <Self as ::serverust_core::ApiError>::message(&self),
                });
                (
                    status,
                    ::serverust_core::__private::axum::Json(body),
                ).into_response()
            }
        }
    };

    expanded.into()
}

// --- #[dynamo_table(...)] -----------------------------------------------------

/// Atributo da macro `#[dynamo_table(name, pk = "...", sk = "...")]`.
struct DynamoTableAttr {
    name: LitStr,
    pk: LitStr,
    sk: Option<LitStr>,
}

impl Parse for DynamoTableAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let mut pk: Option<LitStr> = None;
        let mut sk: Option<LitStr> = None;
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "pk" => pk = Some(input.parse::<LitStr>()?),
                "sk" => sk = Some(input.parse::<LitStr>()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("chave desconhecida `{other}` em #[dynamo_table] (use pk, sk)"),
                    ));
                }
            }
        }
        let pk = pk.ok_or_else(|| {
            syn::Error::new(name.span(), "#[dynamo_table] requer `pk = \"<campo>\"`")
        })?;
        Ok(DynamoTableAttr { name, pk, sk })
    }
}

/// Macro de atributo `#[dynamo_table("TableName", pk = "field", sk = "field"?)]`.
///
/// Aplica-se sobre uma struct e emite `impl serverust_telemetry::dynamo::DynamoTable`
/// expondo `TABLE_NAME`, `PK_FIELD` e `SK_FIELD`. A struct original é re-emitida
/// inalterada — o usuário deve declarar `#[derive(Serialize, Deserialize)]`
/// separadamente (a serialização é usada para extrair os valores dos campos).
///
/// Exemplo:
/// ```ignore
/// #[dynamo_table("Wallets", pk = "user_id")]
/// #[derive(serde::Serialize, serde::Deserialize)]
/// struct Wallet { user_id: String, balance: u64 }
/// ```
#[proc_macro_attribute]
pub fn dynamo_table(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = parse_macro_input!(attr as DynamoTableAttr);
    let item_parsed = parse_macro_input!(item as Item);
    let ident = match &item_parsed {
        Item::Struct(s) => s.ident.clone(),
        other => {
            return syn::Error::new(
                other.span(),
                "#[dynamo_table] só pode ser aplicado em struct",
            )
            .to_compile_error()
            .into();
        }
    };
    let table = attrs.name;
    let pk = attrs.pk;
    let sk_expr = match attrs.sk {
        Some(s) => quote! { ::core::option::Option::Some(#s) },
        None => quote! { ::core::option::Option::None },
    };

    let expanded = quote! {
        #item_parsed

        impl ::serverust_telemetry::dynamo::DynamoTable for #ident {
            const TABLE_NAME: &'static str = #table;
            const PK_FIELD: &'static str = #pk;
            const SK_FIELD: ::core::option::Option<&'static str> = #sk_expr;
        }
    };
    expanded.into()
}

// --- #[subscriber(...)] / #[publisher(...)] ----------------------------------

/// Atributo da macro `#[subscriber(...)]`.
///
/// Sintaxes suportadas:
///
/// - `#[subscriber(topic = "x")]` — driver default `"kafka"` (back-compat v0.2.0).
/// - `#[subscriber(driver = "kafka", topic = "x")]` — explícito.
/// - `#[subscriber(driver = "sqs", queue = "x")]` — adapter SQS (US-001).
///
/// `topic` e `queue` mapeiam ambos para a string passada ao broker via a trait
/// `Broker` — guarda apenas a chave de inscrição.
struct SubscriberAttr {
    /// Driver alvo: `"kafka"` (default) ou `"sqs"`. Outros valores erram.
    driver: LitStr,
    /// Chave de inscrição: `topic` (kafka) ou `queue` (sqs).
    subscribe_key: LitStr,
    /// `true` quando o flag `fifo` foi declarado. Só válido para `driver = "sqs"`.
    fifo: bool,
    /// Span do flag `fifo` para mensagens de erro precisas.
    fifo_span: Option<proc_macro2::Span>,
    /// Máximo de tentativas para `RetryLayer`. Default 1 (sem retry).
    retry_max: u32,
    /// Base do backoff exponencial em milissegundos. Default 0 (sem espera).
    retry_base_ms: u64,
    /// Nome da fila DLQ (US-008). `None` quando o subscriber não declara DLQ.
    dlq_queue: Option<LitStr>,
}

impl Parse for SubscriberAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse manual via `Punctuated::<TokenStream, Comma>` não é trivial
        // porque `retry = exponential(max = 5, base = "100ms")` não é um
        // `Meta` — `exponential(...)` é uma `Expr::Call`. Usamos `syn::Meta`
        // para a maioria dos campos e fazemos parsing especial do `retry`
        // através de uma expressão.
        let mut driver: Option<LitStr> = None;
        let mut topic: Option<LitStr> = None;
        let mut queue: Option<LitStr> = None;
        let mut topic_span: Option<proc_macro2::Span> = None;
        let mut queue_span: Option<proc_macro2::Span> = None;
        let mut fifo = false;
        let mut fifo_span: Option<proc_macro2::Span> = None;
        let mut retry_max: u32 = 1;
        let mut retry_base_ms: u64 = 0;
        let mut dlq_queue: Option<LitStr> = None;

        while !input.is_empty() {
            // Token de chave: identificador.
            if input.peek(syn::Ident) {
                let key: Ident = input.parse()?;
                let key_str = key.to_string();
                // Flag `fifo` sem valor.
                if key_str == "fifo" && !input.peek(Token![=]) {
                    fifo = true;
                    fifo_span = Some(key.span());
                } else {
                    input.parse::<Token![=]>()?;
                    match key_str.as_str() {
                        "driver" => driver = Some(input.parse()?),
                        "topic" => {
                            topic_span = Some(key.span());
                            topic = Some(input.parse()?);
                        }
                        "queue" => {
                            queue_span = Some(key.span());
                            queue = Some(input.parse()?);
                        }
                        "dlq" => dlq_queue = Some(input.parse()?),
                        "retry" => {
                            // Aceita apenas `exponential(max = N, base = "Tms")`.
                            let fn_name: Ident = input.parse()?;
                            if fn_name != "exponential" {
                                return Err(syn::Error::new(
                                    fn_name.span(),
                                    "retry só suporta `exponential(max = N, base = \"Tms\")`",
                                ));
                            }
                            let content;
                            syn::parenthesized!(content in input);
                            // Parse pares `key = value` separados por vírgula.
                            let mut got_max: Option<u32> = None;
                            let mut got_base: Option<u64> = None;
                            while !content.is_empty() {
                                let arg_key: Ident = content.parse()?;
                                content.parse::<Token![=]>()?;
                                match arg_key.to_string().as_str() {
                                    "max" => {
                                        let lit: LitInt = content.parse()?;
                                        got_max = Some(lit.base10_parse::<u32>()?);
                                    }
                                    "base" => {
                                        let lit: LitStr = content.parse()?;
                                        got_base = Some(parse_duration_ms(&lit)?);
                                    }
                                    other => {
                                        return Err(syn::Error::new(
                                            arg_key.span(),
                                            format!(
                                                "chave `{other}` desconhecida em exponential (use max, base)"
                                            ),
                                        ));
                                    }
                                }
                                if content.peek(Token![,]) {
                                    content.parse::<Token![,]>()?;
                                }
                            }
                            retry_max = got_max.ok_or_else(|| {
                                syn::Error::new(fn_name.span(), "exponential requer `max = N`")
                            })?;
                            retry_base_ms = got_base.ok_or_else(|| {
                                syn::Error::new(
                                    fn_name.span(),
                                    "exponential requer `base = \"<N>ms\"`",
                                )
                            })?;
                        }
                        other => {
                            return Err(syn::Error::new(
                                key.span(),
                                format!(
                                    "chave desconhecida `{other}` em #[subscriber] (use driver/topic/queue/fifo/retry/dlq)"
                                ),
                            ));
                        }
                    }
                }
            } else {
                return Err(syn::Error::new(
                    input.span(),
                    "esperado identificador (driver/topic/queue/fifo/retry/dlq)",
                ));
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }

        let driver_value = driver
            .as_ref()
            .map(|d| d.value())
            .unwrap_or_else(|| "kafka".to_string());

        match driver_value.as_str() {
            "kafka" => {
                if let Some(span) = queue_span {
                    return Err(syn::Error::new(
                        span,
                        "`queue` é exclusivo do driver `sqs` — use `topic` para kafka",
                    ));
                }
                if let Some(span) = fifo_span {
                    return Err(syn::Error::new(
                        span,
                        "flag `fifo` é exclusivo do driver `sqs`",
                    ));
                }
                let topic = topic.ok_or_else(|| {
                    syn::Error::new(
                        Span::call_site(),
                        "#[subscriber] com driver `kafka` requer `topic = \"...\"`",
                    )
                })?;
                let driver_lit = driver.unwrap_or_else(|| LitStr::new("kafka", Span::call_site()));
                Ok(SubscriberAttr {
                    driver: driver_lit,
                    subscribe_key: topic,
                    fifo: false,
                    fifo_span: None,
                    retry_max,
                    retry_base_ms,
                    dlq_queue,
                })
            }
            "sqs" => {
                if let Some(span) = topic_span {
                    return Err(syn::Error::new(
                        span,
                        "`topic` é exclusivo do driver `kafka` — use `queue` para sqs",
                    ));
                }
                let queue = queue.ok_or_else(|| {
                    syn::Error::new(
                        Span::call_site(),
                        "#[subscriber] com driver `sqs` requer `queue = \"...\"`",
                    )
                })?;
                let driver_lit = driver.unwrap_or_else(|| LitStr::new("sqs", Span::call_site()));
                Ok(SubscriberAttr {
                    driver: driver_lit,
                    subscribe_key: queue,
                    fifo,
                    fifo_span,
                    retry_max,
                    retry_base_ms,
                    dlq_queue,
                })
            }
            other => Err(syn::Error::new(
                driver
                    .as_ref()
                    .map(|d| d.span())
                    .unwrap_or_else(Span::call_site),
                format!("driver `{other}` não suportado em #[subscriber] — use `kafka` ou `sqs`"),
            )),
        }
    }
}

/// Atributo da macro `#[publisher(topic = "...")]` (mesmo schema de
/// [`SubscriberAttr`]).
struct PublisherAttr {
    topic: LitStr,
}

impl Parse for PublisherAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        let mut topic: Option<LitStr> = None;
        for meta in metas {
            let Meta::NameValue(nv) = meta else {
                return Err(syn::Error::new(meta.span(), "esperado `key = valor`"));
            };
            let key = nv
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            match key.as_str() {
                "topic" => topic = Some(expect_lit_str(&nv.value, "topic")?),
                other => {
                    return Err(syn::Error::new(
                        nv.path.span(),
                        format!("chave desconhecida `{other}` em #[publisher] (use topic)"),
                    ));
                }
            }
        }
        let topic = topic.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "#[publisher] requer `topic = \"...\"`")
        })?;
        Ok(PublisherAttr { topic })
    }
}

/// Macro de atributo `#[subscriber(topic = "...")]`.
///
/// Transforma uma `async fn name(event: T) -> Result<R, BrokerError>` em uma
/// unit struct homônima com:
///
/// - `Self::SUBSCRIBE_TOPIC` — tópico de entrada;
/// - `Self::PUBLISH_TOPIC` — `Option<&'static str>` com o tópico de saída
///   quando `#[publisher(topic = "...")]` é empilhado;
/// - `Self::register(router)` — empurra a inscrição no [`EventRouter`].
///
/// A função original é movida para dentro de `register`, eliminando colisão de
/// nomes com a struct emitida (mesmo padrão de `#[get(...)]`).
///
/// # Composição com `#[publisher]`
///
/// `#[subscriber]` deve ser o atributo **mais externo**: ele consome um
/// `#[publisher(topic = "...")]` interno e emite código baseado em
/// `EventRouter::subscribe_publish`. Sem `#[publisher]`, emite
/// `EventRouter::subscribe` para handler ack-only (`Result<(), BrokerError>`).
///
/// Ambas as macros emitem código do builder `EventRouter` — não há registro
/// global mágico em runtime.
#[proc_macro_attribute]
pub fn subscriber(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = parse_macro_input!(attr as SubscriberAttr);
    let mut func = parse_macro_input!(item as ItemFn);

    // Consome um eventual `#[publisher(topic = "...")]` interno.
    let mut publisher_topic: Option<LitStr> = None;
    let mut publisher_error: Option<syn::Error> = None;
    let mut kept_attrs: Vec<syn::Attribute> = Vec::with_capacity(func.attrs.len());
    for a in func.attrs.drain(..) {
        if a.path().is_ident("publisher") {
            match a.parse_args::<PublisherAttr>() {
                Ok(p) => publisher_topic = Some(p.topic),
                Err(err) => publisher_error = Some(err),
            }
        } else {
            kept_attrs.push(a);
        }
    }
    func.attrs = kept_attrs;
    if let Some(err) = publisher_error {
        return err.to_compile_error().into();
    }

    let vis = func.vis.clone();
    func.vis = syn::Visibility::Inherited;
    let fn_name = func.sig.ident.clone();
    let topic_lit = &attrs.subscribe_key;
    let driver_lit = &attrs.driver;
    let is_fifo = attrs.fifo;
    let retry_max_lit = LitInt::new(&attrs.retry_max.to_string(), Span::call_site());
    let retry_base_ms_lit = LitInt::new(&attrs.retry_base_ms.to_string(), Span::call_site());
    let dlq_const_expr = match &attrs.dlq_queue {
        Some(q) => quote! { ::core::option::Option::Some(#q) },
        None => quote! { ::core::option::Option::None },
    };

    // Compile-time guard FIFO: `fifo` flag <=> handler usa SqsFifoMetadata.
    // Inspeciona os parâmetros pelo último segmento do path do tipo.
    let has_fifo_metadata = func.sig.inputs.iter().any(|arg| {
        let FnArg::Typed(pt) = arg else {
            return false;
        };
        let Type::Path(tp) = &*pt.ty else {
            return false;
        };
        tp.path
            .segments
            .last()
            .map(|s| s.ident == "SqsFifoMetadata")
            .unwrap_or(false)
    });

    if attrs.fifo && !has_fifo_metadata {
        let span = attrs.fifo_span.unwrap_or_else(|| fn_name.span());
        return syn::Error::new(
            span,
            "subscriber FIFO requer um parâmetro do tipo `SqsFifoMetadata` na assinatura — \
             adicione `meta: SqsFifoMetadata` ou remova o flag `fifo`",
        )
        .to_compile_error()
        .into();
    }

    if !attrs.fifo && has_fifo_metadata && attrs.driver.value() == "sqs" {
        return syn::Error::new(
            fn_name.span(),
            "`SqsFifoMetadata` só pode ser usado em subscribers FIFO — \
             adicione o flag `fifo` em `#[subscriber(driver = \"sqs\", queue = \"...\", fifo)]`",
        )
        .to_compile_error()
        .into();
    }

    // Extrai `T` do primeiro parâmetro `event: T`.
    let event_ty = match func.sig.inputs.iter().next() {
        Some(FnArg::Typed(pt)) => (*pt.ty).clone(),
        Some(FnArg::Receiver(r)) => {
            return syn::Error::new(
                r.span(),
                "#[subscriber] não pode ser aplicado em métodos com `self`",
            )
            .to_compile_error()
            .into();
        }
        None => {
            return syn::Error::new(
                fn_name.span(),
                "#[subscriber] requer um parâmetro `event: T`",
            )
            .to_compile_error()
            .into();
        }
    };

    let publish_const = if let Some(t) = &publisher_topic {
        quote! { ::core::option::Option::Some(#t) }
    } else {
        quote! { ::core::option::Option::None }
    };

    let register_body = if let Some(pub_topic) = &publisher_topic {
        quote! {
            #func
            router.subscribe_publish::<#event_ty, _, _, _>(
                Self::SUBSCRIBE_TOPIC,
                #pub_topic,
                #fn_name,
            )
        }
    } else {
        quote! {
            #func
            router.subscribe_with::<#event_ty, _, _>(Self::SUBSCRIBE_TOPIC, #fn_name)
        }
    };

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #vis struct #fn_name;

        impl #fn_name {
            pub const SUBSCRIBE_TOPIC: &'static str = #topic_lit;
            pub const DRIVER: &'static str = #driver_lit;
            pub const IS_FIFO: bool = #is_fifo;
            pub const PUBLISH_TOPIC: ::core::option::Option<&'static str> = #publish_const;
            pub const RETRY_MAX_ATTEMPTS: u32 = #retry_max_lit;
            pub const RETRY_BASE_MS: u64 = #retry_base_ms_lit;
            pub const DLQ_QUEUE: ::core::option::Option<&'static str> = #dlq_const_expr;

            pub fn register(
                router: ::serverust_events::router::EventRouter,
            ) -> ::serverust_events::router::EventRouter {
                #register_body
            }
        }
    };

    expanded.into()
}

/// Macro de atributo `#[publisher(topic = "...")]` — marcador empilhável.
///
/// Não pode ser usada isoladamente: emite código apenas quando consumida pelo
/// `#[subscriber(...)]` mais externo. Aplicada sozinha, gera erro de compilação
/// orientando a colocação correta.
#[proc_macro_attribute]
pub fn publisher(_attr: TokenStream, _item: TokenStream) -> TokenStream {
    syn::Error::new(
        Span::call_site(),
        "#[publisher] precisa ser empilhado dentro de #[subscriber(...)] — \
         coloque #[subscriber(...)] como o atributo mais externo",
    )
    .to_compile_error()
    .into()
}

// --- #[kafka_consumer(...)] ---------------------------------------------------

/// Atributo da macro de consumer Kafka.
///
/// Aceita `topic = "..."` (obrigatório), `group = "..."` (obrigatório),
/// `batch_size = N` (opcional, default `0` = não usado) e `dlq = "..."`
/// (opcional, reservado para futura integração). `topic`/`group` são
/// expostos como constantes associadas na struct gerada.
struct KafkaConsumerAttr {
    topic: LitStr,
    group: LitStr,
    batch_size: Option<LitInt>,
    dlq: Option<LitStr>,
}

impl Parse for KafkaConsumerAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        let mut topic: Option<LitStr> = None;
        let mut group: Option<LitStr> = None;
        let mut batch_size: Option<LitInt> = None;
        let mut dlq: Option<LitStr> = None;

        for meta in metas {
            let Meta::NameValue(nv) = meta else {
                return Err(syn::Error::new(meta.span(), "esperado `key = valor`"));
            };
            let key = nv
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            match key.as_str() {
                "topic" => topic = Some(expect_lit_str(&nv.value, "topic")?),
                "group" => group = Some(expect_lit_str(&nv.value, "group")?),
                "batch_size" => batch_size = Some(expect_lit_int(&nv.value, "batch_size")?),
                "dlq" => dlq = Some(expect_lit_str(&nv.value, "dlq")?),
                other => {
                    return Err(syn::Error::new(
                        nv.path.span(),
                        format!(
                            "chave desconhecida `{other}` em #[kafka_consumer] (use topic, group, batch_size, dlq)"
                        ),
                    ));
                }
            }
        }

        Ok(KafkaConsumerAttr {
            topic: topic.ok_or_else(|| {
                syn::Error::new(
                    Span::call_site(),
                    "#[kafka_consumer] requer `topic = \"...\"`",
                )
            })?,
            group: group.ok_or_else(|| {
                syn::Error::new(
                    Span::call_site(),
                    "#[kafka_consumer] requer `group = \"...\"`",
                )
            })?,
            batch_size,
            dlq,
        })
    }
}

/// Converte literal de duração tipo `"100ms"` ou `"2s"` em milissegundos.
fn parse_duration_ms(lit: &LitStr) -> syn::Result<u64> {
    let s = lit.value();
    let s = s.trim();
    let (num_part, unit, factor): (&str, &str, u64) = if let Some(p) = s.strip_suffix("ms") {
        (p, "ms", 1)
    } else if let Some(p) = s.strip_suffix("s") {
        (p, "s", 1000)
    } else {
        return Err(syn::Error::new(
            lit.span(),
            "duração deve terminar em `ms` ou `s` (ex: \"100ms\", \"2s\")",
        ));
    };
    let n: u64 = num_part.trim().parse().map_err(|_| {
        syn::Error::new(
            lit.span(),
            format!("número inválido antes de `{unit}` em duração: `{s}`"),
        )
    })?;
    Ok(n.saturating_mul(factor))
}

fn expect_lit_str(expr: &Expr, key: &str) -> syn::Result<LitStr> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = expr
    {
        Ok(s.clone())
    } else {
        Err(syn::Error::new(
            expr.span(),
            format!("`{key}` precisa de literal string"),
        ))
    }
}

fn expect_lit_int(expr: &Expr, key: &str) -> syn::Result<LitInt> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Int(n), ..
    }) = expr
    {
        Ok(n.clone())
    } else {
        Err(syn::Error::new(
            expr.span(),
            format!("`{key}` precisa de literal inteiro"),
        ))
    }
}

fn last_path_segment(ty: &Type) -> Option<&PathSegment> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last()
    } else {
        None
    }
}

fn generic_inner<'a>(seg: &'a PathSegment, expected: &str) -> Option<&'a Type> {
    if seg.ident != expected {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

/// `State<Arc<X>>` → `Some(X)`.
fn state_arc_inner(ty: &Type) -> Option<Type> {
    let state_seg = last_path_segment(ty)?;
    let arc_ty = generic_inner(state_seg, "State")?;
    let arc_seg = last_path_segment(arc_ty)?;
    generic_inner(arc_seg, "Arc").cloned()
}

/// Macro de atributo `#[kafka_consumer(topic = "...", group = "...", batch_size = N, dlq = "...")]`.
///
/// Transforma uma função `async fn nome(record: KafkaRecord<T>, State(svc): State<Arc<Svc>>, ...)`
/// em uma unit struct homônima que implementa
/// `serverust_core::events::EventHandler<aws_lambda_events::event::kafka::KafkaEvent>`.
///
/// O handler emitido:
/// - decodifica os registros via `KafkaRecord::from_kafka_event`;
/// - filtra por `topic` (descartando registros de tópicos não declarados);
/// - resolve parâmetros `State<Arc<T>>` a partir do `Container` compartilhado;
/// - invoca a função do usuário para cada registro e propaga o erro como `EventError`.
///
/// `topic`, `group`, `batch_size` e `dlq` ficam disponíveis como constantes
/// associadas (`Self::TOPIC`, `Self::GROUP`, `Self::BATCH_SIZE`, `Self::DLQ`).
#[proc_macro_attribute]
pub fn kafka_consumer(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = parse_macro_input!(attr as KafkaConsumerAttr);
    let mut func = parse_macro_input!(item as ItemFn);

    let vis = func.vis.clone();
    // O `#func` é re-emitido como item local dentro de `fn handle(...)`. Itens
    // aninhados em corpos de função não aceitam `pub`/`pub(crate)` — remover.
    func.vis = syn::Visibility::Inherited;
    let fn_name = func.sig.ident.clone();
    let topic_lit = &attrs.topic;
    let group_lit = &attrs.group;
    let batch_size_lit = attrs
        .batch_size
        .clone()
        .unwrap_or_else(|| LitInt::new("0", Span::call_site()));
    let dlq_const_ty;
    let dlq_const_val;
    if let Some(d) = &attrs.dlq {
        dlq_const_ty = quote! { &'static str };
        dlq_const_val = quote! { #d };
    } else {
        dlq_const_ty = quote! { ::core::option::Option<&'static str> };
        dlq_const_val = quote! { ::core::option::Option::None };
    }

    // Classifica os parâmetros: `State<Arc<X>>` → DI; o demais é o record param.
    let mut state_inner_types: Vec<Type> = Vec::new();
    let mut record_ty: Option<Type> = None;
    let mut call_args: Vec<proc_macro2::TokenStream> = Vec::new();

    for input in func.sig.inputs.iter() {
        let pt = match input {
            FnArg::Typed(pt) => pt,
            FnArg::Receiver(r) => {
                return syn::Error::new(
                    r.span(),
                    "#[kafka_consumer] não pode ser aplicado em métodos com `self`",
                )
                .to_compile_error()
                .into();
            }
        };

        if let Some(inner) = state_arc_inner(&pt.ty) {
            let idx = state_inner_types.len();
            let var = Ident::new(&format!("__svr_state_{idx}"), Span::call_site());
            state_inner_types.push(inner);
            call_args.push(quote! { #var });
        } else if record_ty.is_none() {
            // primeiro parâmetro não-State é o record
            record_ty = Some((*pt.ty).clone());
            call_args.push(quote! { __svr_record });
        } else {
            return syn::Error::new(
                pt.ty.span(),
                "parâmetro inesperado em handler #[kafka_consumer]: use `record: KafkaRecord<T>` e `State<Arc<T>>` para DI",
            )
            .to_compile_error()
            .into();
        }
    }

    let Some(record_ty) = record_ty else {
        return syn::Error::new(
            fn_name.span(),
            "handler #[kafka_consumer] precisa de um parâmetro `record: KafkaRecord<T>`",
        )
        .to_compile_error()
        .into();
    };

    // Constrói as resoluções `let __svr_state_i = State(ctx.get::<X>()...);`.
    let state_resolutions = state_inner_types.iter().enumerate().map(|(i, x)| {
        let var = Ident::new(&format!("__svr_state_{i}"), Span::call_site());
        quote! {
            let __svr_arc = ctx
                .get::<#x>()
                .unwrap_or_else(|| ::core::panic!(
                    "kafka_consumer: service Arc<{}> não registrado no Container",
                    ::core::any::type_name::<#x>(),
                ));
            let #var = ::serverust_core::__private::axum::extract::State(__svr_arc);
        }
    });

    let fn_name_call = fn_name.clone();

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #vis struct #fn_name;

        impl #fn_name {
            pub const TOPIC: &'static str = #topic_lit;
            pub const GROUP: &'static str = #group_lit;
            pub const BATCH_SIZE: usize = #batch_size_lit;
            pub const DLQ: #dlq_const_ty = #dlq_const_val;
        }

        impl ::serverust_core::events::EventHandler<
            ::aws_lambda_events::event::kafka::KafkaEvent,
        > for #fn_name {
            async fn handle(
                &self,
                __svr_event: ::aws_lambda_events::event::kafka::KafkaEvent,
                ctx: &::serverust_core::Container,
            ) -> ::core::result::Result<(), ::serverust_core::events::EventError> {
                #func

                let __svr_records: ::std::vec::Vec<#record_ty> =
                    ::serverust_events::kafka::KafkaRecord::from_kafka_event(&__svr_event)
                        .map_err(|__e| ::serverust_core::events::EventError(::std::string::ToString::to_string(&__e)))?;

                for __svr_record in __svr_records {
                    if __svr_record.topic != #topic_lit {
                        continue;
                    }
                    #( #state_resolutions )*
                    #fn_name_call ( #( #call_args ),* )
                        .await
                        .map_err(|__e| ::serverust_core::events::EventError(
                            ::std::format!("{:?}", __e)
                        ))?;
                }
                ::core::result::Result::Ok(())
            }
        }
    };

    expanded.into()
}
