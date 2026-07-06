# Synoptikon Catalogue Entry Draft

Status: Implemented in Synoptikon integration branch
Target file: `../mnemosyne/synoptikon-products.toml`
Scope: Catalogue and packaging artefact coordination

This document records the DASObjectStore catalogue shape consumed by the
Mnemosyne repository. The release server binary and Trunk web bundle are now
packageable local artefacts when built from this repository.

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
- `binary` names the `dasobjectstore-server` binary produced by
  `cargo build --release -p dasobjectstore-cli --bin dasobjectstore-server`.
- `web_bundle` names the Trunk output produced by running `trunk build --release`
  in `crates/dasobjectstore-gui-web`.

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

The coordinated Synoptikon integration must keep these pieces in place:

- Synoptikon product build/restart support for the `../DASObjectStore` path;
- entitlement provisioning for `entitlement_product_code = "dasobjectstore"`;
- Mneion storage endpoint export and governance-domain binding support for
  DASObjectStore-native endpoints.
