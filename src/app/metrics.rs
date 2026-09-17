//! View-local metric state. No networking, no name-only sample lookup.
use crate::{
    evidence::{Coverage, Evidence, Unknown},
    kube::metrics::{self, Batch, Pins, Sample},
    resources::{Object, store::Store},
};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};

pub struct Cache {
    pub revision: u64,
    pub status: Result<Coverage, Unknown>,
    entries: BTreeMap<String, Evidence<Sample>>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            revision: 0,
            status: Err(Unknown::NotReported),
            entries: BTreeMap::new(),
        }
    }
}
impl Cache {
    pub fn apply(&mut self, pins: Pins, batch: Result<Batch, Unknown>, store: &Store) {
        self.entries.clear();
        self.revision += 1;
        self.status = match batch {
            Err(reason) => Err(reason),
            Ok(mut batch) => {
                batch.coverage.truncated |= store.objects.len() > pins.len();
                for (slot, pin) in pins {
                    let Some(object) = store.objects.get(&slot).filter(|o| o.uid == pin.uid) else {
                        continue;
                    };
                    if let Some(mut sample) = batch.samples.remove(&slot) {
                        if let Err(reason) = metrics::correlate(&sample, &pin, object) {
                            sample.value = Err(reason);
                        }
                        self.entries.insert(pin.uid, sample);
                    } else {
                        batch.coverage.omitted += 1;
                    }
                }
                Ok(batch.coverage)
            }
        };
    }
    pub fn expire(&mut self) {
        for sample in self.entries.values_mut() {
            if sample.value.is_ok() && sample.current(Utc::now(), metrics::TTL).is_err() {
                sample.value = Err(Unknown::Stale);
                self.revision += 1;
            }
        }
    }
    pub fn sample(&self, object: &Object) -> Result<&Evidence<Sample>, Unknown> {
        self.status?;
        let sample = self.entries.get(&object.uid).ok_or(Unknown::NotReported)?;
        sample.current(Utc::now(), metrics::TTL)?;
        Ok(sample)
    }
    pub fn amount(&self, object: &Object, cpu: bool) -> Result<f64, Unknown> {
        let sample = self.sample(object)?.current(Utc::now(), metrics::TTL)?;
        let pick = |u: &metrics::Usage| if cpu { u.cpu } else { u.memory };
        if let Some(node) = &sample.node {
            return pick(node);
        }
        let list = object
            .value
            .pointer("/spec/containers")
            .and_then(serde_json::Value::as_array)
            .filter(|a| !a.is_empty())
            .ok_or(Unknown::NotReported)?;
        let mut expected = BTreeSet::new();
        for container in list {
            expected.insert(container["name"].as_str().ok_or(Unknown::Malformed)?);
        }
        if let Some(inits) = object
            .value
            .pointer("/spec/initContainers")
            .and_then(serde_json::Value::as_array)
        {
            for c in inits.iter().filter(|c| c["restartPolicy"] == "Always") {
                expected.insert(c["name"].as_str().ok_or(Unknown::Malformed)?);
            }
        }
        for path in [
            "/status/initContainerStatuses",
            "/status/ephemeralContainerStatuses",
        ] {
            if let Some(statuses) = object
                .value
                .pointer(path)
                .and_then(serde_json::Value::as_array)
            {
                for c in statuses
                    .iter()
                    .filter(|c| c.pointer("/state/running").is_some())
                {
                    expected.insert(c["name"].as_str().ok_or(Unknown::Malformed)?);
                }
            }
        }
        let mut total = 0.0;
        for name in expected {
            total += pick(sample.containers.get(name).ok_or(Unknown::Partial)?)?;
        }
        if total.is_finite() {
            Ok(total)
        } else {
            Err(Unknown::Malformed)
        }
    }
    /// Usage as a percentage of the Pod's own effective request or limit. A zero
    /// denominator ("no container specified this resource at all") is a distinct
    /// not-applicable result, never a fabricated 0% or an infinity.
    pub fn percentage(&self, object: &Object, cpu: bool, of_limit: bool) -> Result<f64, Unknown> {
        let usage = self.amount(object, cpu)?;
        let field = if of_limit { "limits" } else { "requests" };
        let denominator = crate::resources::accounting::pod_effective(&object.value, field, cpu)?;
        if denominator == 0.0 {
            return Err(Unknown::ZeroDenominator);
        }
        Ok(usage / denominator * 100.0)
    }
    pub fn summary(&self) -> String {
        match self.status {
            Err(reason) => format!("Metrics UNKNOWN: {reason}"),
            Ok(coverage) => {
                let known = self
                    .entries
                    .values()
                    .filter(|s| s.current(Utc::now(), metrics::TTL).is_ok())
                    .count();
                format!(
                    "Metrics {}: {known}/{} fresh; omitted={} malformed={}",
                    if coverage.partial() {
                        "PARTIAL"
                    } else {
                        "sampled"
                    },
                    self.entries.len(),
                    coverage.omitted,
                    coverage.malformed
                )
            }
        }
    }
    pub fn report(&self, object: &Object) -> String {
        let display = |v: Result<f64, Unknown>| match v {
            Ok(n) => n.to_string(),
            Err(r) => format!("UNKNOWN ({r})"),
        };
        let mut report = format!(
            "{}\nTarget {}/{} UID={}\nCPU cores: {}\nMemory bytes: {}\n",
            self.summary(),
            object.kind,
            object.name,
            object.uid,
            display(self.amount(object, true)),
            display(self.amount(object, false))
        );
        if let Ok(evidence) = self.sample(object) {
            report.push_str(&format!(
                "Origin: {:?}; source timestamp: {:?}; received: {}\n",
                evidence.observation.origin,
                evidence.observation.source_at,
                evidence.observation.received_at
            ));
            if let Ok(sample) = &evidence.value {
                for (name, usage) in &sample.containers {
                    report.push_str(&format!(
                        "  {name}: CPU={} cores; memory={} bytes\n",
                        display(usage.cpu),
                        display(usage.memory)
                    ));
                }
            }
        }
        report
    }
}
impl crate::resources::Metrics for Cache {
    fn usage(&self, object: &Object, cpu: bool) -> Result<f64, Unknown> {
        self.amount(object, cpu)
    }
    fn percentage(&self, object: &Object, cpu: bool, of_limit: bool) -> Result<f64, Unknown> {
        self.percentage(object, cpu, of_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kube::metrics::{Pin, decode};
    use serde_json::json;
    fn object(uid: &str) -> Object {
        Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"p","namespace":"n","uid":uid,"creationTimestamp":(Utc::now()-chrono::Duration::minutes(5)).to_rfc3339()},"spec":{"containers":[{"name":"main"}]}}),
        )
    }
    fn batch() -> Batch {
        decode(json!({"items":[{"metadata":{"name":"p","namespace":"n"},"timestamp":Utc::now().to_rfc3339(),"window":"15s","containers":[{"name":"main","usage":{"cpu":"250m","memory":"64Mi"}}]}]}), false).expect("sample")
    }
    #[test]
    fn percentage_zero_denominator_is_distinct_from_unknown_usage() {
        let obj = Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"p","namespace":"n","uid":"u1","creationTimestamp":(Utc::now()-chrono::Duration::minutes(5)).to_rfc3339()},"spec":{"containers":[{"name":"main","resources":{"requests":{"cpu":"500m"}}}]}}),
        );
        let pin = Pin {
            uid: obj.uid.clone(),
            created: obj.created,
        };
        let pins = Pins::from([("n/p".into(), pin)]);
        let mut store = Store::new(10, 100000);
        store.apply(obj, false);
        let mut cache = Cache::default();
        cache.apply(pins.clone(), Ok(batch()), &store);
        // Usage 250m / request 500m = 50%.
        assert_eq!(
            cache.percentage(&store.objects["n/p"], true, false),
            Ok(50.0)
        );
        // No limit specified anywhere: a real zero denominator, not fabricated 0%/inf.
        assert_eq!(
            cache.percentage(&store.objects["n/p"], true, true),
            Err(Unknown::ZeroDenominator)
        );
        cache.apply(pins, Err(Unknown::Forbidden), &store);
        assert_eq!(
            cache.percentage(&store.objects["n/p"], true, false),
            Err(Unknown::Forbidden)
        );
    }
    #[test]
    fn uid_pins_and_replacement_never_share_samples() {
        let mut store = Store::new(10, 100000);
        let old = object("old");
        let pins = Pins::from([(
            "n/p".into(),
            Pin {
                uid: old.uid.clone(),
                created: old.created,
            },
        )]);
        store.apply(old, false);
        let mut cache = Cache::default();
        cache.apply(pins.clone(), Ok(batch()), &store);
        assert_eq!(cache.amount(&store.objects["n/p"], true), Ok(0.25));
        store.apply(object("new"), false);
        assert_eq!(
            cache.amount(&store.objects["n/p"], true),
            Err(Unknown::NotReported)
        );
        cache.apply(pins, Ok(batch()), &store);
        assert!(cache.entries.is_empty());
    }
    #[test]
    fn missing_container_is_partial_not_a_lower_total_and_errors_replace_values() {
        let mut obj = object("old");
        let pin = Pin {
            uid: obj.uid.clone(),
            created: obj.created,
        };
        obj.value["spec"]["containers"]
            .as_array_mut()
            .expect("containers")
            .push(json!({"name":"missing"}));
        let mut store = Store::new(10, 100000);
        store.apply(obj, false);
        let pins = Pins::from([("n/p".into(), pin)]);
        let mut cache = Cache::default();
        cache.apply(pins.clone(), Ok(batch()), &store);
        assert_eq!(
            cache.amount(&store.objects["n/p"], true),
            Err(Unknown::Partial)
        );
        cache.apply(pins, Err(Unknown::Forbidden), &store);
        assert_eq!(
            cache.amount(&store.objects["n/p"], true),
            Err(Unknown::Forbidden)
        );
    }
    #[test]
    fn source_window_before_birth_future_and_stale_samples_are_unknown() {
        let obj = object("old");
        let mut pin = Pin {
            uid: obj.uid.clone(),
            created: obj.created,
        };
        let mut batch = batch();
        let sample = batch.samples.get_mut("n/p").expect("sample");
        sample.value.as_mut().expect("value").window_start =
            obj.created.expect("birth") - chrono::Duration::seconds(1);
        assert_eq!(metrics::correlate(sample, &pin, &obj), Err(Unknown::Stale));
        sample.value.as_mut().expect("value").window_start = Utc::now();
        sample.observation.source_at = Some(Utc::now() - chrono::Duration::seconds(61));
        assert_eq!(metrics::correlate(sample, &pin, &obj), Err(Unknown::Stale));
        sample.observation.source_at = Some(Utc::now());
        pin.uid = "different".into();
        assert_eq!(
            metrics::correlate(sample, &pin, &obj),
            Err(Unknown::TargetReplaced)
        );
    }
}
