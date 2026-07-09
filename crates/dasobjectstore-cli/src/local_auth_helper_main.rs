use dasobjectstore_gui_api::PamLocalPasswordAuthenticator;
use std::io::Read;
use std::process::ExitCode;

const HELPER_BYPASS_ENV: &str = "DASOBJECTSTORE_LOCAL_AUTH_HELPER_BYPASS";
const PROSOPIKON_HELPER_BYPASS_ENV: &str = "PROSOPIKON_LOCAL_AUTH_HELPER_BYPASS";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(LocalAuthHelperError::InvalidCredentials) => ExitCode::from(1),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), LocalAuthHelperError> {
    let mut args = std::env::args().skip(1);
    let mut service_name = "dasobjectstore".to_string();
    let username = match args.next() {
        Some(flag) if flag == "--service" => {
            service_name = args
                .next()
                .ok_or(LocalAuthHelperError::Usage("--service requires a value"))?;
            args.next()
        }
        Some(username) => Some(username),
        None => None,
    }
    .ok_or(LocalAuthHelperError::Usage("username is required"))?;

    if args.next().is_some() {
        return Err(LocalAuthHelperError::Usage(
            "unexpected argument after username",
        ));
    }

    let mut password = String::new();
    std::io::stdin().read_to_string(&mut password)?;
    std::env::set_var(HELPER_BYPASS_ENV, "1");
    std::env::set_var(PROSOPIKON_HELPER_BYPASS_ENV, "1");
    PamLocalPasswordAuthenticator::new(service_name)
        .authenticate(&username, &password)
        .map_err(|err| match err {
            dasobjectstore_gui_api::LocalPasswordAuthError::InvalidCredentials => {
                LocalAuthHelperError::InvalidCredentials
            }
            other => LocalAuthHelperError::Backend(other.to_string()),
        })
}

#[derive(Debug)]
enum LocalAuthHelperError {
    Usage(&'static str),
    InvalidCredentials,
    Backend(String),
    Io(std::io::Error),
}

impl std::fmt::Display for LocalAuthHelperError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "usage error: {message}"),
            Self::InvalidCredentials => write!(formatter, "invalid local username or password"),
            Self::Backend(message) => write!(formatter, "local auth backend failed: {message}"),
            Self::Io(err) => write!(formatter, "local auth helper IO failed: {err}"),
        }
    }
}

impl std::error::Error for LocalAuthHelperError {}

impl From<std::io::Error> for LocalAuthHelperError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
