//! Container tipado de Dependency Injection.
//!
//! Armazena serviços como `Arc<T: ?Sized>` chaveados pelo `TypeId` do próprio
//! `Arc<T>`. Resolução é typesafe em compile-time via [`axum::extract::FromRef`]:
//! qualquer handler que precise de `State<Arc<dyn Trait>>` recebe o `Arc`
//! clonado a partir deste container.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::FromRef;

/// State do `axum::Router`: mapa `TypeId` → `Arc<dyn Any>` carregando os
/// serviços registrados via [`crate::App::provide`].
#[derive(Clone, Default)]
pub struct Container {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insere (ou substitui) `Arc<T>` no container. Chave é o `TypeId` de
    /// `Arc<T>`, então `Arc<dyn TraitA>` e `Arc<dyn TraitB>` ocupam slots
    /// distintos mesmo apontando para a mesma implementação concreta.
    pub fn insert<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) {
        self.services
            .insert(TypeId::of::<Arc<T>>(), Arc::new(value));
    }

    /// Recupera um `Arc<T>` previamente inserido, ou `None` se ausente.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let any_arc = self.services.get(&TypeId::of::<Arc<T>>())?;
        any_arc.downcast_ref::<Arc<T>>().cloned()
    }
}

// Blanket: qualquer `Arc<T>` pode ser extraído do Container via axum's State.
// Possível pelo orphan rule porque `Container` é local e aparece como
// parâmetro de tipo da trait `FromRef`.
impl<T: ?Sized + Send + Sync + 'static> FromRef<Container> for Arc<T> {
    fn from_ref(container: &Container) -> Arc<T> {
        container.get::<T>().unwrap_or_else(|| {
            panic!(
                "service Arc<{}> not registered in App container",
                std::any::type_name::<T>()
            )
        })
    }
}

/// Marker trait emitida por `#[injectable]`. Não impõe método; apenas
/// confirma intenção de injeção e habilita verificações de tipo nos testes.
pub trait Injectable {}
