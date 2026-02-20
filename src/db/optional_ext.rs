use rusqlite::Error;

/// Extension trait for optional query results.
pub(super) trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, Error>;
}

impl<T> OptionalExt<T> for Result<T, Error> {
    fn optional(self) -> Result<Option<T>, Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
