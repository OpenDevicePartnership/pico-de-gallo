use embedded_hal::delay::DelayNs;
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Delay;

impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        thread::sleep(Duration::from_nanos(ns.into()));
    }
}
