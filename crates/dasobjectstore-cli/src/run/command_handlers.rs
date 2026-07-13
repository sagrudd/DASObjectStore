use super::*;

fn parse_disk_roots(values: &[String]) -> Result<Vec<DiskCopyRoot>, CliError> {
    values
        .iter()
        .map(|value| {
            let (disk_id, root_path) =
                value
                    .split_once('=')
                    .ok_or_else(|| CliError::InvalidDiskRootMapping {
                        value: value.clone(),
                    })?;
            let disk_id = DiskId::new(disk_id).map_err(|_| CliError::InvalidDiskRootMapping {
                value: value.clone(),
            })?;
            if root_path.is_empty() {
                return Err(CliError::InvalidDiskRootMapping {
                    value: value.clone(),
                });
            }

            Ok(DiskCopyRoot::new(disk_id, root_path))
        })
        .collect()
}

pub(crate) fn run_object_inspect(
    args: &ObjectInspectArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let summary = read_object_inspect(args.live_sqlite_path(), args.object_id())?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &summary)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_inspect_summary(&summary, writer)?;
    }

    Ok(())
}

pub(crate) fn run_object_export(
    args: &ObjectExportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let disk_roots = parse_disk_roots(args.disk_roots())?;
    let request = ObjectExportRequest::new(
        args.live_sqlite_path(),
        args.object_id().clone(),
        args.destination(),
        disk_roots,
    );
    let report = export_settled_object(&request)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_export_report(&report, writer)?;
    }

    Ok(())
}

pub(crate) fn run_object_put(
    args: &ObjectPutArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let disk_roots = parse_disk_roots(args.disk_roots())?;
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path));
    let response = client.object_put(DaemonObjectPutRequest {
        object_id: args.object_id().to_string(),
        source_path: args.source().to_path_buf(),
        ssd_root: args.ssd_root().to_path_buf(),
        disk_roots: disk_roots
            .into_iter()
            .map(|root| format!("{}={}", root.disk_id, root.root_path.display()))
            .collect(),
        copies: args.copies(),
        object_type: args.object_type(),
    })?;
    let report = response.report;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_put_report(&report, writer)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_disk_roots;

    #[test]
    fn parse_disk_roots_rejects_missing_separator() {
        assert!(parse_disk_roots(&["disk-a".to_string()]).is_err());
    }

    #[test]
    fn parse_disk_roots_rejects_invalid_disk_id() {
        assert!(parse_disk_roots(&[" =/tmp/disk".to_string()]).is_err());
    }

    #[test]
    fn parse_disk_roots_rejects_empty_root() {
        assert!(parse_disk_roots(&["disk-a=".to_string()]).is_err());
    }

    #[test]
    fn parse_disk_roots_preserves_order_and_paths() {
        let roots =
            parse_disk_roots(&["disk-a=/tmp/a".to_string(), "disk-b=relative/b".to_string()])
                .expect("valid disk root mappings");
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].disk_id.as_str(), "disk-a");
        assert_eq!(roots[0].root_path.to_string_lossy(), "/tmp/a");
        assert_eq!(roots[1].disk_id.as_str(), "disk-b");
        assert_eq!(roots[1].root_path.to_string_lossy(), "relative/b");
    }
}

pub(crate) fn run_service_render_compose(
    args: &ServiceRenderComposeArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .stores_file()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let definitions = read_store_registry(&registry_path)?;
    let layout = plan_store_service_layout(&definitions)?;
    let request = ComposeRenderRequest {
        project_name: args.project_name().to_string(),
        ssd_metadata_path: args.ssd_metadata_path().to_string_lossy().to_string(),
        hdd_data_path: args.hdd_data_path().to_string_lossy().to_string(),
        store_bindings: layout.bucket_bindings,
    };
    let rendered = match args.provider() {
        ObjectServiceProviderId::Garage => {
            let provider = GarageProvider::new(GarageProviderConfig {
                service_name: args.service_name().to_string(),
                image: args.image().to_string(),
                bind_address: args.bind_address().to_string(),
                api_port: args.api_port(),
                rpc_port: garage_derived_port(args.api_port(), 1)?,
                web_port: garage_derived_port(args.api_port(), 2)?,
                admin_port: garage_derived_port(args.api_port(), 3)?,
                config_path: args.config_path().to_string_lossy().into_owned(),
                ..GarageProviderConfig::default()
            });
            provider.render_compose(&request)?
        }
        ObjectServiceProviderId::Rustfs => {
            let service = ComposeServiceConfig::new(
                args.provider(),
                args.service_name(),
                args.image(),
                args.api_port(),
            )
            .with_bind_address(args.bind_address());
            render_compose(&request, &service)?
        }
    };

    writer.write_all(rendered.compose_yaml.as_bytes())?;

    Ok(())
}

fn garage_derived_port(api_port: u16, offset: u16) -> Result<u16, ObjectServiceError> {
    api_port.checked_add(offset).ok_or_else(|| {
        ObjectServiceError::InvalidConfiguration(
            "Garage API port must leave room for RPC, web, and admin ports".to_string(),
        )
    })
}

pub(crate) fn run_mnemosyne_export(
    args: &MnemosyneExportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let storage_definition =
        export_mneion_storage_definition(&MneionStorageDefinitionRequest::new(
            args.object_store_id(),
            args.display_name(),
            args.provider(),
            args.endpoint(),
        ))?;
    let mut binding_request =
        MneionBindingSnippetRequest::new(args.object_store_id(), args.governance_domain_id());
    if let Some(note) = args.note() {
        binding_request = binding_request.with_note(note);
    }
    let binding_snippet = export_mneion_binding_snippet(&binding_request)?;

    serde_json::to_writer_pretty(
        &mut *writer,
        &serde_json::json!({
            "storage_definition": storage_definition,
            "binding_snippet": binding_snippet,
        }),
    )?;
    writer.write_all(b"\n")?;

    Ok(())
}

pub(crate) fn run_mnemosyne_validate_nas_nfs_endpoint(
    args: &MnemosyneValidateNasNfsEndpointArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let file = File::open(args.definition_file())?;
    let definition: NasNfsEndpointDefinition = serde_json::from_reader(file)?;
    let validated = validate_nas_nfs_endpoint_definition(&definition)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &validated)?;
        writer.write_all(b"\n")?;
    } else {
        write_nas_nfs_endpoint_validation_report(&validated, writer)?;
    }

    Ok(())
}

#[cfg(feature = "debug-commands")]
pub(crate) fn run_pool_mark_clean(
    args: &PoolMarkerArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let marker =
        PoolStateMarker::clean_eject(args.pool_id().clone(), args.recorded_at_utc().to_string());
    record_pool_state_marker_at(args.live_sqlite_path(), &marker)
        .map_err(|err| CliError::MetadataMarker(err.to_string()))?;
    writeln!(writer, "Marked pool {} clean", args.pool_id())?;

    Ok(())
}

#[cfg(feature = "debug-commands")]
pub(crate) fn run_pool_mark_dirty(
    args: &PoolMarkerArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let marker =
        PoolStateMarker::dirty_attach(args.pool_id().clone(), args.recorded_at_utc().to_string());
    record_pool_state_marker_at(args.live_sqlite_path(), &marker)
        .map_err(|err| CliError::MetadataMarker(err.to_string()))?;
    writeln!(writer, "Marked pool {} dirty", args.pool_id())?;

    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    LinuxProbeProvider::system().probe()
}

#[cfg(target_os = "macos")]
pub(crate) fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    MacosProbeProvider::system().probe()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    Err(ProbeError::UnsupportedPlatform {
        platform: std::env::consts::OS.to_string(),
    })
}
