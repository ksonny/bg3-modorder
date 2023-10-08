#[derive(Debug)]
pub enum Bg3ModError {
    PathNotDirectory,
    AppDataNotFound,
    AppDataDetectionNotSupported,
}

impl std::fmt::Display for Bg3ModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bg3ModError::PathNotDirectory => write!(f, "Provided path is not a directory"),
            Bg3ModError::AppDataNotFound => write!(f, "Failed to locate bg3 app data"),
            Bg3ModError::AppDataDetectionNotSupported => write!(f, "bg3 app data detection not supported on your system, use --bg3-path option"),
        }
    }
}

impl std::error::Error for Bg3ModError {}
