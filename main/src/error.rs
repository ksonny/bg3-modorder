#[derive(Debug)]
pub enum Bg3ModError {
    PathNotDirectory,
}

impl std::fmt::Display for Bg3ModError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Bg3ModError::PathNotDirectory => write!(f, "Provided path is not a directory"),
        }
    }
}

impl std::error::Error for Bg3ModError {}
