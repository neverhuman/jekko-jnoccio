#[path = "responses_body.rs"]
mod responses_body;

pub use crate::providers::completion_common::parse_raw_completion;
pub use responses_body::{ResponsesClient, build_body};
