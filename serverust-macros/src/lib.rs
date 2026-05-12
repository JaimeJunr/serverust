//! Macros de roteamento do serverust.
//!
//! Cada macro (`#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]`) transforma
//! a função anotada em uma struct unit com o mesmo nome, implementando
//! `serverust_core::IntoRoute`. O handler original permanece, mas como item
//! aninhado dentro de `into_route`, de modo que o nome público passa a ser a
//! struct registrável via `App::route(handler)`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, ExprLit, Fields, Ident, Item, ItemFn, Lit, LitInt, LitStr, Meta,
    Token, Type, parse_macro_input, parse_quote, punctuated::Punctuated, spanned::Spanned,
};

fn make_route(method: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    let path = parse_macro_input!(attr as LitStr);
    let func = parse_macro_input!(item as ItemFn);

    let vis = func.vis.clone();
    let fn_name = func.sig.ident.clone();
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

    let expanded = quote! {
        #[allow(non_camel_case_types)]
        #vis struct #fn_name;

        impl ::serverust_core::IntoRoute for #fn_name {
            fn into_route(self) -> ::serverust_core::Route {
                #func

                let operation = ::serverust_core::__private::utoipa::openapi::path::OperationBuilder::new()
                    .operation_id(Some(#fn_name_str))
                    .response(
                        "200",
                        ::serverust_core::__private::utoipa::openapi::ResponseBuilder::new()
                            .description("OK")
                            .build(),
                    )
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
