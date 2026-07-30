use super::*;

#[derive(Debug, Args)]
pub struct TrustArgs {
    #[command(subcommand)]
    command: TrustCommand,
}

impl TrustArgs {
    pub fn command(&self) -> &TrustCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum TrustCommand {
    /// Enrol first-use appliance TLS trust without requesting a password.
    Enroll(TrustEnrollArgs),
    /// Inspect trust for one appliance endpoint.
    Inspect(TrustInspectArgs),
    /// List enrolled appliance trust records.
    List(TrustListArgs),
    /// Remove one appliance trust record.
    Remove(TrustRemoveArgs),
    /// Replace a changed certificate using an independently verified fingerprint.
    Rotate(TrustRotateArgs),
    /// Repair certificate trust, renew the session, and optionally configure S3.
    Repair(TrustRepairArgs),
}

#[derive(Debug, Args)]
pub struct TrustEnrollArgs {
    host_or_ip: String,
    #[arg(long, default_value_t = crate::authenticate::DEFAULT_APPLIANCE_HTTPS_PORT)]
    https_port: u16,
    /// PEM CA certificate that must verify the presented endpoint certificate.
    #[arg(
        long,
        conflicts_with = "trust_fingerprint",
        required_unless_present = "trust_fingerprint"
    )]
    ca_cert: Option<PathBuf>,
    /// Independently verified SHA-256 fingerprint of the presented leaf certificate.
    #[arg(long, conflicts_with = "ca_cert", required_unless_present = "ca_cert")]
    trust_fingerprint: Option<String>,
}

impl TrustEnrollArgs {
    pub fn host_or_ip(&self) -> &str {
        &self.host_or_ip
    }
    pub fn https_port(&self) -> u16 {
        self.https_port
    }
    pub fn ca_cert(&self) -> Option<&Path> {
        self.ca_cert.as_deref()
    }
    pub fn trust_fingerprint(&self) -> Option<&str> {
        self.trust_fingerprint.as_deref()
    }
}

#[derive(Debug, Args)]
pub struct TrustInspectArgs {
    host_or_ip: String,
    #[arg(long, default_value_t = crate::authenticate::DEFAULT_APPLIANCE_HTTPS_PORT)]
    https_port: u16,
    #[arg(long)]
    json: bool,
}

impl TrustInspectArgs {
    pub fn host_or_ip(&self) -> &str {
        &self.host_or_ip
    }
    pub fn https_port(&self) -> u16 {
        self.https_port
    }
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct TrustListArgs {
    #[arg(long)]
    json: bool,
}

impl TrustListArgs {
    pub fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Args)]
pub struct TrustRemoveArgs {
    appliance_id: String,
    /// Confirm removal without an interactive prompt.
    #[arg(long)]
    yes: bool,
}

impl TrustRemoveArgs {
    pub fn appliance_id(&self) -> &str {
        &self.appliance_id
    }
    pub fn yes(&self) -> bool {
        self.yes
    }
}

#[derive(Debug, Args)]
pub struct TrustRotateArgs {
    appliance_id: String,
    #[arg(long)]
    trust_fingerprint: String,
}

impl TrustRotateArgs {
    pub fn appliance_id(&self) -> &str {
        &self.appliance_id
    }
    pub fn trust_fingerprint(&self) -> &str {
        &self.trust_fingerprint
    }
}

#[derive(Debug, Args)]
pub struct TrustRepairArgs {
    host_or_ip: String,
    #[arg(long, default_value_t = crate::authenticate::DEFAULT_APPLIANCE_HTTPS_PORT)]
    https_port: u16,
    #[arg(long)]
    username: Option<String>,
    #[arg(long)]
    store: String,
    #[arg(long)]
    set_s3_config: bool,
    #[arg(long, requires = "set_s3_config")]
    s3_profile: Option<String>,
    #[arg(long, requires = "set_s3_config")]
    force: bool,
    #[arg(long, requires = "set_s3_config")]
    no_verify_s3: bool,
}

impl TrustRepairArgs {
    pub fn host_or_ip(&self) -> &str {
        &self.host_or_ip
    }
    pub fn https_port(&self) -> u16 {
        self.https_port
    }
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
    pub fn store(&self) -> &str {
        &self.store
    }
    pub fn as_authenticate_args(&self) -> AuthenticateArgs {
        AuthenticateArgs {
            host_or_ip: self.host_or_ip.clone(),
            object_store: self.store.clone(),
            https_port: self.https_port,
            username: self.username.clone(),
            ca_cert: None,
            tls_server_name: None,
            trust_fingerprint: None,
            session_lifetime_seconds: None,
            json: false,
            set_s3_config: self.set_s3_config,
            s3_profile: self.s3_profile.clone(),
            force: self.force,
            no_verify_s3: self.no_verify_s3,
        }
    }
}
