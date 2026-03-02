use std::time::Instant;

pub struct OperationTimer<'a> {
    label: &'a str,
    start: Instant,
}

impl<'a> OperationTimer<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, start: Instant::now() }
    }
}

impl<'a> Drop for OperationTimer<'a> {
    fn drop(&mut self) {
        log::info!("⏲️ {} took {:?}", self.label, self.start.elapsed());
    }
}
