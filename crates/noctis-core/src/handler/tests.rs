//! Story 5.6 Matrix 41: `HandlerError`.

use std::io;

use super::*;

fn fails() -> HandlerResult<()> {
    Err(io::Error::new(io::ErrorKind::NotFound, "no such guild"))?;
    Ok(())
}

#[test]
fn question_mark_converts() {
    let err = fails().unwrap_err();
    assert_eq!(err.to_string(), "no such guild");
    let inner = err.as_error().downcast_ref::<io::Error>().unwrap();
    assert_eq!(inner.kind(), io::ErrorKind::NotFound);
    assert!(format!("{err:?}").contains("NotFound"));
}

#[test]
fn new_wraps() {
    let err = HandlerError::new(io::Error::other("boom"));
    assert_eq!(err.to_string(), "boom");
    assert!(err.as_error().downcast_ref::<io::Error>().is_some());
    assert_eq!(
        format!("{err:?}"),
        format!("{:?}", io::Error::other("boom"))
    );
}
