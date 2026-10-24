//! GitHub (<https://docs.github.com/rest>): release assets of public repositories found by
//! repository search. Unauthenticated (60 core requests and 10 searches per hour/minute), so
//! the adapter looks at only a few repositories per search and surfaces a rate limit as a
//! typed error instead of hammering the API.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, tidy, year_of, Category, Hit, Source};

/// Repositories whose latest release is looked at per search.
const MAX_REPOS: usize = 4;
/// Assets offered per release.
const MAX_ASSETS: usize = 2;

/// GitHub adapter.
pub struct Github {
    base: String,
    http: Http,
}

impl Github {
    /// Talks to `api.github.com`.
    pub fn new() -> Self {
        Self::with_base("https://api.github.com", client_for(Source::Github))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Github {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        self.http
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }
}

impl Default for Github {
    fn default() -> Self {
        Self::new()
    }
}

/// One repository from the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Repo {
    pub full_name: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub html_url: String,
    pub default_branch: Option<String>,
}

pub(crate) fn parse_repos(body: &Value) -> Vec<Repo> {
    body.get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|r| {
                    r.get("private").and_then(Value::as_bool) != Some(true)
                        && r.get("disabled").and_then(Value::as_bool) != Some(true)
                })
                .filter_map(|r| {
                    Some(Repo {
                        full_name: jstr(r.get("full_name")?)?.to_string(),
                        description: r.get("description").and_then(jstr).map(str::to_string),
                        license: r
                            .pointer("/license/spdx_id")
                            .and_then(jstr)
                            .filter(|l| *l != "NOASSERTION")
                            .map(str::to_string),
                        html_url: jstr(r.get("html_url")?)?.to_string(),
                        default_branch: r.get("default_branch").and_then(jstr).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Does this SPDX id name an open-source (or public-domain) licence? Used for games, where only
/// repositories that clearly declare one are offered. `NOASSERTION`, `LicenseRef-*`, Creative
/// Commons non-software licences (except CC0) and anything with a non-commercial or
/// no-derivatives clause are not.
pub(crate) fn is_open_licence(spdx: &str) -> bool {
    let u = spdx.trim().to_ascii_uppercase();
    if u.is_empty()
        || u == "NOASSERTION"
        || u == "OTHER"
        || u.starts_with("LICENSEREF")
        || u.contains("-NC")
        || u.contains("-ND")
    {
        return false;
    }
    if u.starts_with("CC-") {
        return false;
    }
    true
}

/// Signatures and checksums are not what someone searching for software wants to download.
fn is_companion(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        ".sig", ".asc", ".sha256", ".sha512", ".sha1", ".md5", ".pem", ".sbom", ".json", ".txt",
        ".yml", ".yaml",
    ]
    .iter()
    .any(|e| n.ends_with(e))
}

/// One open-source repository as a hit: the source archive of its default branch (GitHub builds
/// it on request, so it has no size), the licence and the owner as publisher. Only repositories
/// that declare an open licence are offered.
pub(crate) fn repo_hit(repo: &Repo) -> Option<Hit> {
    let spdx = repo.license.as_deref().filter(|l| is_open_licence(l))?;
    let name = repo.full_name.rsplit('/').next().unwrap_or(&repo.full_name);
    let branch = repo.default_branch.as_deref().unwrap_or("HEAD");
    let url = format!(
        "https://github.com/{}/archive/{}.zip",
        repo.full_name, branch
    );
    let mut h = Hit::new(
        Source::Github,
        &format!("repo:{}", repo.full_name),
        &repo.full_name,
        &url,
    );
    h.filename = Some(format!("{name}.zip"));
    h.page_url = Some(repo.html_url.clone());
    h.license = Some(spdx.to_string());
    h.publisher = repo.full_name.split('/').next().map(str::to_string);
    h.authors = h.publisher.iter().cloned().collect();
    h.summary = repo.description.as_deref().map(|d| tidy(d, 300));
    Some(h)
}

pub(crate) fn parse_release(repo: &Repo, release: &Value) -> Vec<Hit> {
    let tag = release.get("tag_name").and_then(jstr).unwrap_or("latest");
    let year = release.get("published_at").and_then(jstr).and_then(year_of);
    release
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| {
            let mut sorted: Vec<&Value> = assets.iter().collect();
            // Riptide is a Windows-first app: installers and archives for Windows come first
            sorted.sort_by_key(|a| {
                let n = a
                    .get("name")
                    .and_then(jstr)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if n.contains("win")
                    || n.ends_with(".exe")
                    || n.ends_with(".msi")
                    || n.ends_with(".msix")
                {
                    0
                } else if n.ends_with(".zip") || n.ends_with(".7z") || n.ends_with(".pdf") {
                    1
                } else {
                    2
                }
            });
            sorted
                .into_iter()
                .filter_map(|a| {
                    let name = jstr(a.get("name")?)?;
                    let url = jstr(a.get("browser_download_url")?).filter(|u| is_web_url(u))?;
                    if is_companion(name) {
                        return None;
                    }
                    let title = format!("{} {tag} - {name}", repo.full_name);
                    let mut h = Hit::new(
                        Source::Github,
                        &format!("{}@{tag}/{name}", repo.full_name),
                        &title,
                        url,
                    );
                    h.size = a.get("size").and_then(Value::as_u64);
                    h.filename = Some(name.to_string());
                    h.page_url = Some(repo.html_url.clone());
                    h.license = repo.license.clone();
                    h.year = year;
                    h.version = release.get("tag_name").and_then(jstr).map(str::to_string);
                    h.publisher = repo.full_name.split('/').next().map(str::to_string);
                    h.authors = repo
                        .full_name
                        .split('/')
                        .next()
                        .map(|o| vec![o.to_string()])
                        .unwrap_or_default();
                    h.summary = repo.description.as_deref().map(|d| tidy(d, 300));
                    Some(h)
                })
                .take(MAX_ASSETS)
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl SearchSource for Github {
    fn source(&self) -> Source {
        Source::Github
    }

    fn max_queries(&self) -> usize {
        1
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        if category == Category::OpenSource {
            return self.repos(&q, wanted).await;
        }
        if category != Category::Games {
            return self.run(&q, wanted, false).await;
        }
        // Games: repositories tagged `game`, then `games` when the first topic found nothing
        let first = self.run(&format!("{q} topic:game"), wanted, true).await?;
        if !first.is_empty() {
            return Ok(first);
        }
        self.run(&format!("{q} topic:games"), wanted, true).await
    }
}

impl Github {
    /// Open-source projects: the repositories themselves (no release needed).
    async fn repos(&self, q: &str, wanted: usize) -> Result<Vec<Hit>> {
        let url = format!(
            "{}/search/repositories?q={}&sort=stars&order=desc&per_page={}",
            self.base,
            enc(q),
            (wanted * 2).clamp(6, 30)
        );
        let body = self.http.json(|| self.request(&url)).await?;
        if body.get("items").is_none() {
            return Err(ResearchError::Parse("no items field".into()));
        }
        let mut hits: Vec<Hit> = parse_repos(&body).iter().filter_map(repo_hit).collect();
        hits.truncate(wanted);
        Ok(hits)
    }

    /// One repository search plus the latest release of the first few repositories.
    /// `open_only`: skip repositories that do not declare an open-source licence.
    async fn run(&self, q: &str, wanted: usize, open_only: bool) -> Result<Vec<Hit>> {
        let url = format!(
            "{}/search/repositories?q={}&sort=stars&order=desc&per_page={}",
            self.base,
            enc(q),
            if open_only { 12 } else { MAX_REPOS * 2 }
        );
        let body = self.http.json(|| self.request(&url)).await?;
        if body.get("items").is_none() {
            return Err(ResearchError::Parse("no items field".into()));
        }
        let mut hits = Vec::new();
        let repos = parse_repos(&body)
            .into_iter()
            .filter(|r| !open_only || r.license.as_deref().is_some_and(is_open_licence));
        for repo in repos.take(MAX_REPOS) {
            if hits.len() >= wanted {
                break;
            }
            let rurl = format!("{}/repos/{}/releases/latest", self.base, repo.full_name);
            match self.http.json(|| self.request(&rurl)).await {
                Ok(rel) => hits.extend(parse_release(&repo, &rel)),
                // 404: the repository has no release; anything else: skip this repository
                Err(ResearchError::RateLimited { retry_after }) if hits.is_empty() => {
                    return Err(ResearchError::RateLimited { retry_after })
                }
                Err(e) => tracing::debug!(error = %e, "github repo skipped"),
            }
        }
        hits.truncate(wanted);
        Ok(hits)
    }
}
