//! Small shared evidence vocabulary. Unknownness and provenance never become values.
use chrono::{DateTime, Utc};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unknown {
    Unavailable,
    Forbidden,
    NotFound,
    NotReported,
    Stale,
    Unsupported,
    Partial,
    TargetReplaced,
    TransportError,
    Malformed,
    TimedOut,
    ZeroDenominator,
}
impl std::fmt::Display for Unknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    MetricsApi,
    ObjectApi,
    ObjectWatch,
    EventsApi,
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub source_at: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    received_tick: Instant,
    pub origin: Origin,
}
impl Observation {
    pub fn new(origin: Origin, source_at: Option<DateTime<Utc>>) -> Self {
        Self {
            source_at,
            received_at: Utc::now(),
            received_tick: Instant::now(),
            origin,
        }
    }
    /// Receipt TTL is monotonic; a backwards wall clock cannot keep a sample alive.
    /// A source >5s in the future is not usable current evidence either.
    pub fn freshness(&self, now: DateTime<Utc>, ttl: Duration) -> Result<(), Unknown> {
        if self.received_tick.elapsed() > ttl {
            return Err(Unknown::Stale);
        }
        let time = self.source_at.unwrap_or(self.received_at);
        let age = now.signed_duration_since(time).num_milliseconds();
        if age < -5000 || age as i128 > ttl.as_millis() as i128 {
            Err(Unknown::Stale)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug)]
pub struct Evidence<T> {
    pub value: Result<T, Unknown>,
    pub observation: Observation,
}
impl<T> Evidence<T> {
    pub fn current(&self, now: DateTime<Utc>, ttl: Duration) -> Result<&T, Unknown> {
        let value = self.value.as_ref().map_err(|r| *r)?;
        self.observation.freshness(now, ttl)?;
        Ok(value)
    }
}
/// Bounded summary, not an unbounded bag of API messages or raw bodies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub omitted: usize,
    pub malformed: usize,
    pub truncated: bool,
}
impl Coverage {
    pub fn partial(self) -> bool {
        self.omitted > 0 || self.malformed > 0 || self.truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_zero_is_not_unknown_and_source_age_is_not_receipt_age() {
        let now = Utc::now();
        let mut e = Evidence {
            value: Ok(0.0),
            observation: Observation::new(Origin::MetricsApi, Some(now)),
        };
        let ttl = Duration::from_secs(60);
        assert_eq!(e.current(now, ttl), Ok(&0.0));
        e.observation.source_at = Some(now - chrono::Duration::seconds(61));
        assert_eq!(e.current(now, ttl), Err(Unknown::Stale));
        e.observation.source_at = Some(now + chrono::Duration::seconds(6));
        assert_eq!(e.current(now, ttl), Err(Unknown::Stale));
        e.value = Err(Unknown::Forbidden);
        assert_eq!(e.current(now, ttl), Err(Unknown::Forbidden));
        e.value = Ok(2.0);
        e.observation.source_at = Some(now);
        e.observation.received_tick = Instant::now() - Duration::from_secs(61);
        assert_eq!(e.current(now, ttl), Err(Unknown::Stale));
    }
    #[test]
    fn partial_coverage_does_not_mean_no_evidence() {
        assert!(!Coverage::default().partial());
        assert!(
            Coverage {
                omitted: 1,
                ..Default::default()
            }
            .partial()
        );
    }
}
