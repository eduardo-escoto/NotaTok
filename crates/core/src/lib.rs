use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// Example function — replace with your real logic.
pub fn greet(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(CoreError::InvalidInput("name cannot be empty".into()));
    }
    Ok(format!("Hello, {name}!"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greet() {
        assert_eq!(greet("world").unwrap(), "Hello, world!");
    }

    #[test]
    fn test_greet_empty_name() {
        assert!(greet("").is_err());
    }
}
