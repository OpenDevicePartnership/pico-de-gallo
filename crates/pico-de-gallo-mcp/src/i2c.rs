//! i2c tools — populated in a later task.

use rmcp::tool_router;

use crate::GalloMcp;

#[tool_router(router = i2c_router, vis = "pub(crate)")]
impl GalloMcp {}
