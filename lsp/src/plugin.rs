use lapce_core::plugin::PluginBufferInfo;

pub trait Plugin {
    fn new_buffer(&self, buffer_info: &PluginBufferInfo);
}
