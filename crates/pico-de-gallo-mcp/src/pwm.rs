//! pwm tools — populated in a later task.

use rmcp::tool_router;

use crate::GalloMcp;

#[tool_router(router = pwm_router, vis = "pub(crate)")]
impl GalloMcp {}
