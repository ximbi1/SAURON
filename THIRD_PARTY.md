# Third-party components

SAURON is independently implemented. Sofka is a behavioral reference, not an affiliate.
Research source: https://github.com/nklmilojevic/sofka at
2024e5921cf9cb06fd02f666630f97bae5e24eb3 (MIT OR Apache-2.0, Nikola Milojević).
No Sofka code/artwork is incorporated; notes paraphrase public behavior.

Direct component families: kube/k8s-openapi (Apache-2.0), Ratatui/Crossterm (MIT),
Tokio/tokio-util (MIT), serde/serde_json, clap, anyhow, futures-util, regex and TOML
(MIT OR Apache-2.0), tracing/tracing-subscriber (MIT). Cargo.lock is authoritative
for versions. Before distribution generate a complete transitive license inventory and
include required notices, including TLS and platform dependencies. This file is not an
exhaustive distribution license bundle.

M5 adds `http-body-util` 0.1 as a direct dependency (already transitive through kube),
licensed MIT, to stream bounded HTTP bodies without kube's eager error-body collection.

The isolated-kind metrics fixture references the unmodified upstream Kubernetes SIGs
[metrics-server v0.8.1 manifest](https://github.com/kubernetes-sigs/metrics-server/releases/tag/v0.8.1),
licensed [Apache-2.0](https://github.com/kubernetes-sigs/metrics-server/blob/v0.8.1/LICENSE).
The local Kustomize patch only adds a test-cluster kubelet certificate exception.
