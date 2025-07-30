

use super::EnvFilter;

pub struct Suppress {
    restore: EnvFilter,
}

impl Suppress {
    pub fn new() -> Self {
        let handle = "console";
        let suppress = EnvFilter::default();
        let conf = &crate::config::get().logger;
        let restore = crate::logging::get()
            .reload
            .current(handle)
            .unwrap_or_else(|| EnvFilter::try_new(&conf.level).unwrap_or_default());

        crate::logging::get()
            .reload
            .reload(&suppress, Some(&[handle]))
            .expect("log filter reloaded");

        Self { restore }
    }
}

impl Drop for Suppress {
    fn drop(&mut self) {
        crate::logging::get()
            .reload
            .reload(&self.restore, Some(&["console"]))
            .expect("log filter reloaded");
    }
}
