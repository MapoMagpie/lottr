use std::time;

#[allow(dead_code)]
pub struct RateLimit {
    pub limit: usize,
    pub interval: time::Duration,
    pub cb: fn(time::Duration) -> bool,
    current: usize,
    last: time::Instant,
    duration: time::Duration,
}

impl RateLimit {
    #[allow(dead_code)]
    pub fn new(limit: usize, interval: time::Duration, cb: fn(time::Duration) -> bool) -> Self {
        Self {
            limit,
            current: 0,
            cb,
            interval,
            last: time::Instant::now(),
            duration: time::Duration::from_secs(0),
        }
    }
    #[allow(dead_code)]
    pub fn wait(&mut self) -> bool {
        let mut ok = false;
        if self.current < self.limit {
            self.current += 1;
            // println!("current: {}", self.current);
        } else {
            self.duration = time::Instant::now() - self.last;
            // println!("duration: {:?}", self.duration);
            if self.duration <= self.interval {
                ok = (self.cb)(self.interval - self.duration);
            }
            self.reset();
        }
        ok
    }

    pub fn reset(&mut self) {
        self.current = 0;
        self.last = time::Instant::now();
        self.duration = time::Duration::from_secs(0);
    }
}

#[cfg(test)]
mod test {
    use std::time;

    use super::RateLimit;

    #[test]
    fn test_rate_limit_sleep() {
        let mut rate_limit = RateLimit::new(
            1,
            time::Duration::from_secs(3),
            |duration: time::Duration| {
                println!("sleeping {:?}", duration);
                std::thread::sleep(duration);
                true
            },
        );
        for _ in 0..10 {
            rate_limit.wait();
            println!("waited");
        }
    }
}
