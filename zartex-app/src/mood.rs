//! `Moods\moodNN.txt`: the environment a Driv3r mission starts with (time of day, weather, music, lighting).
//!
//! What it does: reads the `KEY: value` lines of a mood file (keys are upper-cased, `#` and `//` lines are skipped), and
//! turns the lighting and fog keys into a `city3d::Lighting` for the renderers.
//! Uses: `city3d` (`Lighting`), `glam`. Used by: `main.rs` (loads the mission's mood file), `overview` (shows its values).
//! The `Lighting` it builds is taken by the renderers of `city3d`.

use std::path::Path;

/// The key and value pairs of one mood file, in file order (keys in upper case).
pub struct Mood(Vec<(String, String)>);

impl Mood {
    /// Reads a mood file. `None` when the file cannot be read. Lines without a `:` are ignored.
    pub fn load(path: &Path) -> Option<Mood> {
        let t = std::fs::read_to_string(path).ok()?;
        let mut v = Vec::new();
        for line in t.lines() {
            let l = line.trim();
            if l.starts_with('#') || l.starts_with("//") {
                continue;
            }
            if let Some((k, val)) = l.split_once(':') {
                v.push((k.trim().to_uppercase(), val.trim().trim_matches('"').to_string()));
            }
        }
        Some(Mood(v))
    }

    /// The value of key `k` (exact, upper-case match), if the file has it. The first occurrence wins.
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    /// The first three comma-separated numbers of key `k` as an RGB triple, as written in the file.
    pub fn rgb(&self, k: &str) -> Option<[f32; 3]> {
        let v: Vec<f32> = self.get(k)?.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() >= 3).then(|| [v[0], v[1], v[2]])
    }

    /// The first whitespace-separated word of key `k` as a number.
    pub fn num(&self, k: &str) -> Option<f32> {
        self.get(k)?.split_whitespace().next()?.parse().ok()
    }
}

impl Mood {
    /// Lighting and fog of the mood, if it defines them. `LIGHT_AMBIENT_COLOUR`, `LIGHT_DIRECTIONAL_COLOUR` and
    /// `LIGHT_DIRECTIONAL_VECTOR` are required. The fog keys are optional: the fog distances default to 50 and 1000, the
    /// fog values to 0 and the fog colour to (0.7, 0.75, 0.8).
    pub fn lighting(&self) -> Option<crate::city3d::Lighting> {
        // the direction is three numbers; anything else means the mood has no usable light
        let v = self.get("LIGHT_DIRECTIONAL_VECTOR")?.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>();
        if v.len() < 3 {
            return None;
        }
        Some(crate::city3d::Lighting {
            ambient: self.rgb("LIGHT_AMBIENT_COLOUR")?,
            dir_col: self.rgb("LIGHT_DIRECTIONAL_COLOUR")?,
            dir_vec: glam::Vec3::new(v[0], v[1], v[2]),
            fog_near: self.num("FOG_NEARDISTANCE").unwrap_or(50.0),
            fog_far: self.num("FOG_FARDISTANCE").unwrap_or(1000.0),
            fog_near_v: self.num("FOG_NEARVALUE").unwrap_or(0.0),
            fog_far_v: self.num("FOG_FARVALUE").unwrap_or(0.0),
            fog_col: self.rgb("FOG_COLOUR").unwrap_or([0.7, 0.75, 0.8]),
        })
    }
}
