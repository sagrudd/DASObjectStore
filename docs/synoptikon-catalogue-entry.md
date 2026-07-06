# Synoptikon Catalogue Entry Draft

Status: Draft  
Target file: `../mnemosyne/synoptikon-products.toml`  
Scope: Milestone 13 catalogue planning only

This document records the DASObjectStore catalogue changes that should be made
in the Mnemosyne repository when the coordinated Synoptikon integration change
is ready. It intentionally does not modify `../mnemosyne` from this repository.

## Product Entry

Add this product entry to the `[[products]]` section:

```toml
[[products]]
id = "dasobjectstore"
display_name = "DASObjectStore"
mode = "dual_host"
package_staging = "bundled"
binary = "../DASObjectStore/target/release/dasobjectstore-server"
web_bundle = "../DASObjectStore/crates/dasobjectstore-gui-web/dist"
manifest = "../DASObjectStore/product-manifest.json"
health_path = "/products/dasobjectstore/health"
api_path = "/products/dasobjectstore/api"
port_policy = "catalogue_assigned"
migrations = []
workflow_definitions = []
required_services = ["limen", "keryx"]
entitlement_product_code = "dasobjectstore"
```

Rationale:

- `mode = "dual_host"` matches the product manifest support for standalone and
  `synoptikon_integrated` modes.
- `port_policy = "catalogue_assigned"` keeps integrated deployments behind
  Synoptikon rather than exposing the standalone HTTPS port.
- `required_services = ["limen", "keryx"]` follows the current product manifest
  platform dependencies while DASObjectStore remains an object-style storage
  appliance.
- `binary` names the planned Milestone 14 standalone/server entry point. The
  path should be revisited when the server crate or binary name stabilizes.
- `web_bundle` names the existing Yew crate output path and should be revisited
  when Trunk packaging is wired.

## Package Profile Additions

Add DASObjectStore to integrated profiles where local storage-appliance
management should be available:

```toml
products = [
  "hermeneia",
  "mnematikon",
  "poikilognostikon",
  "phoreus",
  "ergasterion",
  "harmonia-synthesis",
  "xenognostikon",
  "dasobjectstore"
]
```

Add a Monas standalone package profile for non-Synoptikon appliance operation:

```toml
[[package_profiles]]
id = "monas-dasobjectstore-standalone"
display_name = "Monas DASObjectStore Standalone"
profile_kind = "monas_standalone"
inherits = []
products = ["dasobjectstore"]
platform_services = []

[package_profiles.standalone]
host = "monas"
product_root_template = "/opt/<productName>"
package_formats = ["deb", "rpm", "container", "source"]
local_persistence = "json_files_with_sqlite_index"
workflow_provider = "local_hardware"
```

## Port Boundary

The Synoptikon catalogue entry must not set `fixed_port = 8448`.

`8448` is the permanent standalone HTTPS port for customer and client appliance
deployments. Synoptikon-integrated deployments must use Synoptikon's public
listener and a catalogue-assigned internal product port.

## Coordinated Changes Required

Before applying this entry in `../mnemosyne`, confirm or implement:

- the `dasobjectstore-server` binary named in the catalogue;
- the generated Yew bundle path under `crates/dasobjectstore-gui-web/dist`;
- Synoptikon product build/restart support for the `../DASObjectStore` path;
- entitlement provisioning for `entitlement_product_code = "dasobjectstore"`;
- Mneion storage endpoint export and governance-domain binding support for
  DASObjectStore-native endpoints.
