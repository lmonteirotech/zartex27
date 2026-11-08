//! Driv3r mission catalogue (names per city), as listed by the original Zartex.
//! Holds the City enum, the TABLE of mission descriptors (name, mode, city and mission ids), and two helpers: stem turns
//! a mission id into a file stem, and city_of_file turns a file stem back into a city.
//! Exposes City, Descriptor, TABLE, stem and city_of_file. Uses nothing else in this repository.
//! Used by main.rs: the Missions tab groups the mission files by city and mode and names them with stem.
//! city_of_file has no caller in the crate.

/// The three cities of Driv3r.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum City {
    /// Miami.
    Miami,
    /// Nice.
    Nice,
    /// Istanbul.
    Istanbul,
}

impl City {
    /// All cities, in the order the Missions tab lists them.
    pub const ALL: [City; 3] = [City::Miami, City::Nice, City::Istanbul];
    /// Display name of the city.
    pub fn name(self) -> &'static str {
        match self {
            City::Miami => "Miami",
            City::Nice => "Nice",
            City::Istanbul => "Istanbul",
        }
    }
}

/// One catalogue entry: a mission (or a series of parts) as the Missions tab shows it.
pub struct Descriptor {
    /// Name in the list, for example Police HQ.
    pub name: &'static str,
    /// "Undercover", "Take a Ride", "Quick Chase", ...
    pub mode: &'static str,
    pub city: City,
    /// First id is the mission, the rest are its sub-missions.
    pub ids: &'static [u32],
}

// shorthand for one TABLE entry: name, mode, city variant, and the list of ids
macro_rules! d {
    ($n:expr, $m:expr, $c:ident, [$($i:expr),*]) => {
        Descriptor { name: $n, mode: $m, city: City::$c, ids: &[$($i),*] }
    };
}

/// The Driv3r mission catalogue, one entry per mission, grouped by mode and then by city.
pub const TABLE: &[Descriptor] = &[
    // Undercover missions, Miami first, then Nice and Istanbul
    d!("Police HQ", "Undercover", Miami, [1, 101, 102]),
    d!("Lead on Baccus", "Undercover", Miami, [2, 103]),
    d!("The Siege", "Undercover", Miami, [3, 105]),
    d!("Rooftops", "Undercover", Miami, [4, 106, 107]),
    d!("Impress Lomaz", "Undercover", Miami, [5, 108, 109, 121]),
    d!("Gator's Yacht", "Undercover", Miami, [6, 110, 111]),
    d!("The Hit", "Undercover", Miami, [7, 112, 122]),
    d!("Trapped", "Undercover", Miami, [8, 113, 114, 115]),
    d!("Dodge Island", "Undercover", Miami, [9, 116, 117]),
    d!("Retribution", "Undercover", Miami, [10, 118, 119, 120]),
    d!("Welcome to Nice", "Undercover", Nice, [11, 130, 131]),
    d!("Smash and Run", "Undercover", Nice, [13, 134]),
    d!("18-wheeler", "Undercover", Nice, [14, 135, 150]),
    d!("Hijack", "Undercover", Nice, [15, 136]),
    d!("Arms Deal", "Undercover", Nice, [16, 137, 138, 139, 149]),
    d!("Booby Trap", "Undercover", Nice, [17, 140, 151, 152]),
    d!("Calita in Trouble", "Undercover", Nice, [18, 141, 142]),
    d!("Rescue Dubois", "Undercover", Nice, [19, 143, 144]),
    d!("Hunted", "Undercover", Nice, [21, 146, 147, 148]),
    d!("Surveillance", "Undercover", Istanbul, [22, 160, 161, 162]),
    d!("Tanner Escapes", "Undercover", Istanbul, [24, 164, 165, 180]),
    d!("Another Lead", "Undercover", Istanbul, [25, 168, 181]),
    d!("Alleyway", "Undercover", Istanbul, [27, 171, 172]),
    d!("The Chase", "Undercover", Istanbul, [28, 173, 174]),
    d!("Bomb Truck", "Undercover", Istanbul, [30, 176]),
    d!("Chase the Train", "Undercover", Istanbul, [31, 177, 178, 179]),
    // Quick Chase, Quick Getaway, Trail Blazer, Survival, Checkpoint Race, Gate Race and Take a Ride: three cities each
    d!("Quick Chase", "Quick Chase", Miami, [32, 33]),
    d!("Quick Chase", "Quick Chase", Nice, [34, 35]),
    d!("Quick Chase", "Quick Chase", Istanbul, [36, 37]),
    d!("Quick Getaway", "Quick Getaway", Miami, [38, 39, 40]),
    d!("Quick Getaway", "Quick Getaway", Nice, [42, 43, 44]),
    d!("Quick Getaway", "Quick Getaway", Istanbul, [46, 47, 48]),
    d!("Trail Blazer", "Trail Blazer", Miami, [50, 51]),
    d!("Trail Blazer", "Trail Blazer", Nice, [52, 53]),
    d!("Trail Blazer", "Trail Blazer", Istanbul, [54, 55]),
    d!("Survival", "Survival", Miami, [56]),
    d!("Survival", "Survival", Nice, [57]),
    d!("Survival", "Survival", Istanbul, [58]),
    d!("Checkpoint Race", "Checkpoint Race", Miami, [59, 60, 61]),
    d!("Checkpoint Race", "Checkpoint Race", Nice, [62, 63, 64]),
    d!("Checkpoint Race", "Checkpoint Race", Istanbul, [65, 66, 67]),
    d!("Gate Race", "Gate Race", Miami, [71, 72]),
    d!("Gate Race", "Gate Race", Nice, [73, 74]),
    d!("Gate Race", "Gate Race", Istanbul, [75, 76]),
    d!("Take a Ride", "Take a Ride", Miami, [77, 78]),
    d!("Take a Ride", "Take a Ride", Nice, [80, 81]),
    d!("Take a Ride", "Take a Ride", Istanbul, [83, 84]),
];

/// File stem of a mission id, `mission01`, `mission101`.
pub fn stem(id: u32) -> String {
    format!("mission{id:02}")
}

/// City of a mission file, given its stem (for example mission07). Returns None when the name is not a mission name or its id is not in TABLE.
pub fn city_of_file(stem: &str) -> Option<City> {
    let id: u32 = stem.to_lowercase().strip_prefix("mission")?.parse().ok()?;
    TABLE.iter().find(|d| d.ids.contains(&id)).map(|d| d.city)
}
