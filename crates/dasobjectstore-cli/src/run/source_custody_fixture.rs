//! Private CLI test binding; production keeps the fixed daemon catalogue.
use std::cell::RefCell;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;

use dasobjectstore_object_service::{
    CustodyCatalogBinding, ObjectServiceError, StoreRegistryUpdateReport, StoreServiceDefinition,
    StoreServiceLayout,
};

thread_local! {
    static BINDING: RefCell<Option<CustodyCatalogBinding>> = const { RefCell::new(None) };
}

pub(super) struct CustodyFixtureScope {
    previous: Option<CustodyCatalogBinding>,
    // A thread-local binding must be restored on the thread that installed it.
    same_thread: PhantomData<Rc<()>>,
}

impl CustodyFixtureScope {
    pub(super) fn enter(root: &Path) -> Result<Self, ObjectServiceError> {
        let binding = CustodyCatalogBinding::new(root.join("synthetic-custody-catalog.jsonl"))?;
        let previous = BINDING.with(|slot| slot.replace(Some(binding)));
        Ok(Self {
            previous,
            same_thread: PhantomData,
        })
    }
}

impl Drop for CustodyFixtureScope {
    fn drop(&mut self) {
        BINDING.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

pub(super) fn read_store_registry(
    path: impl AsRef<Path>,
) -> Result<Vec<StoreServiceDefinition>, ObjectServiceError> {
    BINDING.with(|slot| match slot.borrow().as_ref() {
        Some(binding) => {
            dasobjectstore_object_service::read_store_registry_with_custody_catalog(path, binding)
        }
        None => dasobjectstore_object_service::read_store_registry(path),
    })
}

pub(super) fn upsert_store_definition(
    path: impl AsRef<Path>,
    definition: StoreServiceDefinition,
) -> Result<StoreRegistryUpdateReport, ObjectServiceError> {
    BINDING.with(|slot| match slot.borrow().as_ref() {
        Some(binding) => {
            dasobjectstore_object_service::upsert_store_definition_with_custody_catalog(
                path, definition, binding,
            )
        }
        None => dasobjectstore_object_service::upsert_store_definition(path, definition),
    })
}

pub(super) fn plan_store_service_layout(
    definitions: &[StoreServiceDefinition],
) -> Result<StoreServiceLayout, ObjectServiceError> {
    BINDING.with(|slot| match slot.borrow().as_ref() {
        Some(binding) => {
            dasobjectstore_object_service::plan_store_service_layout_with_custody_catalog(
                definitions,
                binding,
            )
        }
        None => dasobjectstore_object_service::plan_store_service_layout(definitions),
    })
}
