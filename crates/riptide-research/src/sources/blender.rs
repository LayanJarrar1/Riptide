//! The Blender Foundation's open movies, from a small built-in catalogue. All of them are
//! released under Creative Commons Attribution licences and are hosted at
//! `download.blender.org`; the links and sizes below were checked with `HEAD` requests on
//! 2026-10-20. No network call is made by a search.
//!
//! Only films with a plain, public file on `download.blender.org` are listed. Newer films
//! (Spring, Agent 327, Coffee Run, Hero, Charge, Sprite Fright, Wing It!) are distributed
//! through Blender Studio's account-bound downloads and are not offered here.

use async_trait::async_trait;

use super::{check_query, clamp, SearchSource};
use crate::error::Result;
use crate::types::{Category, Hit, Source};

/// One film of the catalogue.
struct Film {
    id: &'static str,
    title: &'static str,
    year: u16,
    summary: &'static str,
    /// Path below the download host.
    path: &'static str,
    size: u64,
    license: &'static str,
    page: &'static str,
}

const CATALOGUE: [Film; 6] = [
    Film {
        id: "big-buck-bunny",
        title: "Big Buck Bunny (720p)",
        year: 2008,
        summary: "A giant rabbit with a heart bigger than himself takes revenge on three bullying rodents. Blender Foundation open movie \"Peach\".",
        path: "peach/bigbuckbunny_movies/big_buck_bunny_720p_h264.mov.zip",
        size: 416_751_396,
        license: "CC BY 3.0",
        page: "https://peach.blender.org/",
    },
    Film {
        id: "elephants-dream",
        title: "Elephants Dream (1024p)",
        year: 2006,
        summary: "Two strange characters explore a capricious and seemingly infinite machine. The first Blender Foundation open movie, \"Orange\".",
        path: "ED/elephantsdream-1024-h264-st-aac.mov",
        size: 326_632_467,
        license: "CC BY 2.5",
        page: "https://orange.blender.org/",
    },
    Film {
        id: "sintel",
        title: "Sintel (720p)",
        year: 2010,
        summary: "A lonely young woman searches for the baby dragon she befriended. Blender Foundation open movie \"Durian\".",
        path: "demo/movies/Sintel.2010.720p.mkv.zip",
        size: 673_935_592,
        license: "CC BY 3.0",
        page: "https://durian.blender.org/",
    },
    Film {
        id: "tears-of-steel",
        title: "Tears of Steel (720p)",
        year: 2012,
        summary: "Live action science fiction short with visual effects made in Blender. Blender Foundation open movie \"Mango\".",
        path: "demo/movies/ToS/tears_of_steel_720p.mov",
        size: 372_178_639,
        license: "CC BY 3.0",
        page: "https://mango.blender.org/",
    },
    Film {
        id: "cosmos-laundromat",
        title: "Cosmos Laundromat (720p)",
        year: 2015,
        summary: "On a desolate island, a suicidal sheep named Franck meets a mysterious salesman. Blender Foundation open movie \"Gooseberry\".",
        path: "gooseberry/movies/cosmos_laundromat_720p.mp4",
        size: 294_028_179,
        license: "CC BY 4.0",
        page: "https://gooseberry.blender.org/",
    },
    Film {
        id: "caminandes-gran-dillama",
        title: "Caminandes: Gran Dillama",
        year: 2013,
        summary: "Koro the llama tries to reach the fence of the other side of the pasture. Short film by the Blender Institute.",
        path: "demo/movies/caminandes_gran_dillama.mp4.zip",
        size: 125_632_282,
        license: "CC BY 3.0",
        page: "https://www.caminandes.com/",
    },
];

/// Words that match every film.
const GENERIC: &str = "blender foundation open movie film short animation";

/// Blender open movies adapter.
pub struct Blender {
    base: String,
}

impl Blender {
    /// The official download host.
    pub fn new() -> Self {
        Self::with_base("https://download.blender.org")
    }

    /// Serves the catalogue from `base` (tests).
    pub fn with_base(base: &str) -> Self {
        Blender {
            base: base.trim_end_matches('/').into(),
        }
    }

    fn hit(&self, f: &Film) -> Hit {
        let url = format!("{}/{}", self.base, f.path);
        let mut h = Hit::new(Source::Blender, f.id, f.title, &url);
        h.authors = vec!["Blender Foundation".into()];
        h.year = Some(f.year);
        h.summary = Some(f.summary.into());
        h.license = Some(f.license.into());
        h.size = Some(f.size);
        h.page_url = Some(f.page.into());
        h.filename = f.path.rsplit('/').next().map(str::to_string);
        h
    }
}

impl Default for Blender {
    fn default() -> Self {
        Self::new()
    }
}

/// Every query word must occur (case-insensitive) in the title, year, summary or the generic
/// words.
fn matches(f: &Film, words: &[String]) -> bool {
    let hay = format!(
        "{} {} {} {GENERIC}",
        f.title.to_lowercase(),
        f.year,
        f.summary.to_lowercase()
    );
    words.iter().all(|w| hay.contains(w.as_str()))
}

#[async_trait]
impl SearchSource for Blender {
    fn source(&self) -> Source {
        Source::Blender
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        if !matches!(category, Category::All | Category::Video) {
            return Ok(Vec::new());
        }
        let words: Vec<String> = q.split(' ').map(str::to_lowercase).collect();
        Ok(CATALOGUE
            .iter()
            .filter(|f| matches(f, &words))
            .take(clamp(limit))
            .map(|f| self.hit(f))
            .collect())
    }
}
