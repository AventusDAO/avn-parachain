use std::time::Instant;

pub struct Timer<'a> {
    label: &'a str,
    start: Instant,
}

impl<'a> Timer<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, start: Instant::now() }
    }
}

impl<'a> Drop for Timer<'a> {
    fn drop(&mut self) {
        log::info!("⏲️ {} took {:?}", self.label, self.start.elapsed());
    }
}
