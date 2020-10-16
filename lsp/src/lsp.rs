use lapce_core::plugin::PluginBufferInfo;

use crate::plugin::Plugin;

pub struct LspPlugin {}

impl LspPlugin {
    pub fn new() -> LspPlugin {
        LspPlugin {}
    }
}

impl Plugin for LspPlugin {
    fn new_buffer(&self, buffer_info: &PluginBufferInfo) {
        eprintln!("got new buffer");
    }
}
