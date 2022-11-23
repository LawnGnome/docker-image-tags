use std::{collections::BTreeMap, thread};

use chrono::{TimeZone, Utc};
use clap::Parser;
use miette::{IntoDiagnostic, Result};
use reqwest::{blocking::Client, StatusCode};
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
struct Opt {
    #[arg(long, default_value = "hub.docker.com")]
    host: String,

    #[arg(short, long)]
    namespace: String,

    #[arg(short, long)]
    repo: String,
}

#[derive(Deserialize)]
struct Results {
    next: Option<String>,
    results: Vec<Tag>,
}

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct MajorMinor {
    major: u64,
    minor: u64,
}

impl Serialize for MajorMinor {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}.{}", self.major, self.minor))
    }
}

#[derive(Default)]
struct VersionSet {
    versions: BTreeMap<MajorMinor, Version>,
}

impl VersionSet {
    fn maybe_insert(&mut self, mut version: Version) {
        self.versions
            .entry(MajorMinor {
                major: version.major,
                minor: version.minor,
            })
            .and_modify(|value| *value = value.max(&mut version).clone())
            .or_insert(version);
    }
}

impl Serialize for VersionSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_map(self.versions.iter().map(|(k, max)| (k, max.to_string())))
    }
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    let mut versions = VersionSet::default();
    let fetcher = TagFetcher::new(&opt.host, &opt.namespace, &opt.repo);
    for tag_result in fetcher {
        let name = tag_result?.name;
        match lenient_semver::parse(&name) {
            Ok(version) => versions.maybe_insert(version),
            Err(_) => {
                eprintln!("ignoring unparsable version {}", name);
            }
        }
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&versions).into_diagnostic()?
    );

    Ok(())
}

struct TagFetcher {
    client: Client,
    next: Option<String>,
    tags: Vec<Tag>,
}

impl TagFetcher {
    fn new(host: &str, namespace: &str, repo: &str) -> Self {
        Self {
            client: Client::new(),
            next: Some(format!(
                "https://{}/v2/namespaces/{}/repositories/{}/tags?page_size=100",
                host, namespace, repo
            )),
            tags: Vec::new(),
        }
    }

    fn refill_cache(&mut self) -> Result<()> {
        let url = match self.next.take() {
            Some(url) => url,
            None => {
                return Ok(());
            }
        };

        loop {
            let resp = self.client.get(&url).send().into_diagnostic()?;

            // If we got a 429, spin and try again.
            if resp.status() == StatusCode::TOO_MANY_REQUESTS {
                // Spin and try again based on the return header.
                match resp.headers().get("x-retry-after") {
                    Some(ts) => {
                        let retry_after = Utc
                            .timestamp_opt(
                                ts.to_str().into_diagnostic()?.parse().into_diagnostic()?,
                                0,
                            )
                            .single()
                            .ok_or_else(|| {
                                miette::miette!("could not parse x-retry-after {:?}", ts)
                            })?;
                        if let Ok(duration) = (retry_after - Utc::now()).to_std() {
                            thread::sleep(duration);
                        }

                        continue;
                    }
                    None => {
                        miette::bail!("got 429, but not x-retry-after header");
                    }
                }
            }

            // Otherwise, handle any errors and return.
            let resp = resp.error_for_status().into_diagnostic()?;
            let result: Results = resp.json().into_diagnostic()?;
            self.next = result.next;
            self.tags = result.results;
            break;
        }

        Ok(())
    }
}

impl Iterator for TagFetcher {
    type Item = Result<Tag>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.tags.is_empty() {
            if let Err(e) = self.refill_cache() {
                return Some(Err(e));
            }
        }

        self.tags.pop().map(Ok)
    }
}
