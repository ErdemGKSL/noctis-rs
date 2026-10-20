//! Boundary passes: values exactly at a rule's limit compile and validate.
//! One module per area, named like the fixtures' area directories; each test
//! cites its rule id in its name.

#[path = "boundary/msg.rs"]
mod msg;

#[path = "boundary/cid.rs"]
mod cid;

#[path = "boundary/cv2.rs"]
mod cv2;

#[path = "boundary/modal.rs"]
mod modal;
