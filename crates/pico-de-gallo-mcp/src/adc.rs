//! adc tools — populated in a later task.

use rmcp::tool_router;

use crate::GalloMcp;

#[tool_router(router = adc_router, vis = "pub(crate)")]
impl GalloMcp {}
