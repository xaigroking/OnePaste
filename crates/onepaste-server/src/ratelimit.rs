//! Per-IP token buckets, kept in memory only. `X-Forwarded-For` is not trusted by default, so the IP here is the TCP peer address.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// register, invite and key parameter reads with a registration credential
    Register,
    /// failed authentication
    AuthFail,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct Limiter {
    register_per_min: u32,
    auth_fail_per_min: u32,
    buckets: Mutex<HashMap<(Kind, IpAddr), Bucket>>,
}

impl Limiter {
    pub fn new(register_per_min: u32, auth_fail_per_min: u32) -> Limiter {
        Limiter { register_per_min, auth_fail_per_min, buckets: Mutex::new(HashMap::new()) }
    }

    fn capacity(&self, kind: Kind) -> f64 {
        f64::from(match kind {
            Kind::Register => self.register_per_min,
            Kind::AuthFail => self.auth_fail_per_min,
        })
    }

    /// Refills the bucket for the elapsed time, then runs `f` on it.
    fn with_bucket<R>(&self, kind: Kind, ip: IpAddr, f: impl FnOnce(&mut Bucket, f64) -> R) -> R {
        let cap = self.capacity(kind);
        let mut map = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let b = map.entry((kind, ip)).or_insert(Bucket { tokens: cap, last: now });
        let refill = now.duration_since(b.last).as_secs_f64() * cap / 60.0;
        b.tokens = (b.tokens + refill).min(cap);
        b.last = now;
        f(b, cap)
    }

    fn retry_after(tokens: f64, cap: f64) -> u64 {
        let per_token = 60.0 / cap.max(1.0);
        (((1.0 - tokens) * per_token).ceil() as u64).max(1)
    }

    /// Takes one token; when none is left, returns the suggested wait in seconds.
    pub fn take(&self, kind: Kind, ip: IpAddr) -> Result<(), u64> {
        self.with_bucket(kind, ip, |b, cap| {
            if b.tokens >= 1.0 {
                b.tokens -= 1.0;
                Ok(())
            } else {
                Err(Self::retry_after(b.tokens, cap))
            }
        })
    }

    /// Only tells whether the bucket is exhausted, without taking a token.
    pub fn check(&self, kind: Kind, ip: IpAddr) -> Result<(), u64> {
        self.with_bucket(kind, ip, |b, cap| if b.tokens >= 1.0 { Ok(()) } else { Err(Self::retry_after(b.tokens, cap)) })
    }

    /// Failed authentication: takes one token (an empty bucket stays empty).
    pub fn record_failure(&self, ip: IpAddr) {
        self.with_bucket(Kind::AuthFail, ip, |b, _| b.tokens = (b.tokens - 1.0).max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_limit_then_blocks_with_retry_after() {
        let l = Limiter::new(3, 20);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..3 {
            assert!(l.take(Kind::Register, ip).is_ok());
        }
        let wait = l.take(Kind::Register, ip).unwrap_err();
        assert!((1..=60).contains(&wait));
        let other: IpAddr = "10.0.0.2".parse().unwrap();
        assert!(l.take(Kind::Register, other).is_ok());
    }

    #[test]
    fn failures_exhaust_the_auth_bucket() {
        let l = Limiter::new(10, 2);
        let ip: IpAddr = "10.0.0.3".parse().unwrap();
        assert!(l.check(Kind::AuthFail, ip).is_ok());
        l.record_failure(ip);
        l.record_failure(ip);
        assert!(l.check(Kind::AuthFail, ip).is_err());
    }
}
