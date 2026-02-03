use std::time::Instant;

pub struct Web3Timer<'a> {
    label: &'a str,
    start: Instant,
}

impl<'a> Web3Timer<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, start: Instant::now() }
    }
}

impl<'a> Drop for Web3Timer<'a> {
    fn drop(&mut self) {
        log::info!("⏲️ {} took {:?}", self.label, self.start.elapsed());
    }
}
