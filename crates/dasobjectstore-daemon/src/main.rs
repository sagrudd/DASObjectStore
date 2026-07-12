use dasobjectstore_daemon::{
    admin_job_registry_path, appliance_telemetry_state_path, AdminJobRegistry,
    ApplianceTelemetryLoop, ApplianceTelemetryLoopConfig, ApplianceTelemetrySink,
    ApplianceTelemetrySource, DaemonRequestHandler, DaemonRuntimeConfig,
    FileBackedAdminJobRegistry, FileBackedApplianceTelemetrySink,
    FileBackedCapacityAdmissionProvider, GarageServiceController, GarageServiceRuntimeConfig,
    LinuxProcTelemetryCollector, SystemDaemonClock, SystemServiceCommandRunner,
    UnixSocketDaemonServer, DEFAULT_DAEMON_CONFIG_PATH,
};
use dasobjectstore_object_service::{DEFAULT_GARAGE_API_PORT, DEFAULT_GARAGE_CONFIG_PATH};
use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = DaemonArgs::parse(env::args().skip(1))?;
    if args.help {
        print_help();
        return Ok(());
    }

    let config = read_config(&args.config_path)?;
    config.validate().map_err(|err| err.to_string())?;

    if args.check_config {
        println!("Daemon config is valid: {}", args.config_path.display());
        return Ok(());
    }

    let garage =
        GarageServiceController::new(garage_runtime_config(&config)?, SystemServiceCommandRunner)
            .with_capacity_admission_provider(Arc::new(
                FileBackedCapacityAdmissionProvider::for_daemon(&config.state_dir),
            ));
    let admin_job_registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
        &config.state_dir,
    )));
    let interrupted = admin_job_registry
        .mark_interrupted_at_startup(&current_utc_timestamp())
        .map_err(|error| error.to_string())?;
    if interrupted > 0 {
        eprintln!("marked {interrupted} interrupted daemon job(s) failed after restart");
    }
    let handler = DaemonRequestHandler::new_with_admin_job_registry(
        garage,
        SystemDaemonClock,
        admin_job_registry,
    );
    let _telemetry_loop = spawn_appliance_telemetry_loop(&config)?;
    let server = UnixSocketDaemonServer::new(&config.socket_path, handler);
    println!(
        "dasobjectstored listening on {}",
        server.socket_path().display()
    );
    server.serve_forever().map_err(|err| err.to_string())
}

fn spawn_appliance_telemetry_loop(
    config: &DaemonRuntimeConfig,
) -> Result<Option<thread::JoinHandle<()>>, String> {
    if !config.telemetry.enabled {
        return Ok(None);
    }
    let loop_config = ApplianceTelemetryLoopConfig::new(
        config.telemetry.cadence_seconds,
        ApplianceTelemetrySource {
            appliance_id: "local-appliance".to_string(),
            host_id: host_id(),
            hostname: env::var("HOSTNAME")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        },
    )
    .map_err(|err| err.to_string())?;
    let cadence = loop_config.cadence();
    let telemetry_path = appliance_telemetry_state_path(&config.state_dir);

    Ok(Some(thread::spawn(move || {
        let mut telemetry_loop =
            ApplianceTelemetryLoop::new(loop_config, LinuxProcTelemetryCollector::default());
        let mut sink = FileBackedApplianceTelemetrySink::new(telemetry_path);
        loop {
            match telemetry_loop.collect_once(current_utc_timestamp()) {
                Ok(sample_set) => {
                    if let Err(error) = sink.record(&sample_set) {
                        eprintln!("appliance telemetry write failed: {error}");
                    }
                }
                Err(error) => eprintln!("appliance telemetry collection failed: {error}"),
            }
            thread::sleep(cadence);
        }
    })))
}

fn host_id() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local-host".to_string())
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let result = unsafe { libc::gmtime_r(&seconds, tm.as_mut_ptr()) };
    if result.is_null() {
        return "1970-01-01T00:00:00Z".to_string();
    }
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn garage_runtime_config(
    config: &DaemonRuntimeConfig,
) -> Result<GarageServiceRuntimeConfig, String> {
    let config_dir = config.config_path.parent().ok_or_else(|| {
        format!(
            "daemon config path has no parent: {}",
            config.config_path.display()
        )
    })?;
    Ok(GarageServiceRuntimeConfig {
        compose_file: config_dir.join("garage.compose.yml"),
        project_directory: Some(config.state_dir.join("garage")),
        compose_project: "dasobjectstore".to_string(),
        service_name: "garage".to_string(),
        config_path: PathBuf::from(DEFAULT_GARAGE_CONFIG_PATH),
        metadata_path: PathBuf::from("/srv/dasobjectstore/ssd/garage"),
        data_path: PathBuf::from("/srv/dasobjectstore/hdd/garage"),
        endpoint: format!("http://0.0.0.0:{DEFAULT_GARAGE_API_PORT}"),
    })
}

fn read_config(path: &PathBuf) -> Result<DaemonRuntimeConfig, String> {
    let file = File::open(path)
        .map_err(|err| format!("failed to open daemon config {}: {err}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|err| format!("failed to parse daemon config {}: {err}", path.display()))
}

fn print_help() {
    println!("Usage: dasobjectstored [--config <PATH>] [--check-config]");
}

#[derive(Debug)]
struct DaemonArgs {
    config_path: PathBuf,
    check_config: bool,
    help: bool,
}

impl DaemonArgs {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config_path = PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH);
        let mut check_config = false;
        let mut help = false;
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--config requires a path".to_string())?;
                    config_path = PathBuf::from(value);
                }
                "--check-config" => check_config = true,
                "-h" | "--help" => help = true,
                value => return Err(format!("unsupported dasobjectstored argument: {value}")),
            }
        }

        Ok(Self {
            config_path,
            check_config,
            help,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{current_utc_timestamp, garage_runtime_config, host_id, DaemonArgs};
    use dasobjectstore_daemon::DaemonRuntimeConfig;
    use std::path::PathBuf;

    #[test]
    fn parses_config_and_check_flag() {
        let args = DaemonArgs::parse([
            "--config".to_string(),
            "/etc/dasobjectstore/daemon.json".to_string(),
            "--check-config".to_string(),
        ])
        .expect("args parse");

        assert_eq!(
            args.config_path,
            PathBuf::from("/etc/dasobjectstore/daemon.json")
        );
        assert!(args.check_config);
    }

    #[test]
    fn rejects_missing_config_path() {
        let err = DaemonArgs::parse(["--config".to_string()]).expect_err("missing path rejected");

        assert_eq!(err, "--config requires a path");
    }

    #[test]
    fn derives_garage_runtime_paths_from_daemon_config() {
        let config = DaemonRuntimeConfig::linux_packaged();

        let garage = garage_runtime_config(&config).expect("garage config");

        assert_eq!(
            garage.compose_file,
            PathBuf::from("/etc/dasobjectstore/garage.compose.yml")
        );
        assert_eq!(
            garage.project_directory,
            Some(PathBuf::from("/var/lib/dasobjectstore/garage"))
        );
        assert_eq!(
            garage.metadata_path,
            PathBuf::from("/srv/dasobjectstore/ssd/garage")
        );
        assert_eq!(garage.endpoint, "http://0.0.0.0:3900");
    }

    #[test]
    fn daemon_timestamp_uses_utc_rfc3339_shape() {
        let timestamp = current_utc_timestamp();

        assert_eq!(timestamp.len(), 20);
        assert!(timestamp.ends_with('Z'));
        assert_eq!(&timestamp[4..5], "-");
        assert_eq!(&timestamp[10..11], "T");
    }

    #[test]
    fn daemon_host_id_is_nonblank() {
        assert!(!host_id().trim().is_empty());
    }
}
