use rand::seq::SliceRandom;
use rand::RngExt;

/// Generation mode for the password generator view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum GeneratorMode {
    #[default]
    Password,
    Passphrase,
}

/// Word separator for Diceware-style passphrases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum PassphraseSeparator {
    #[default]
    Dash,
    Underscore,
    Space,
    Dot,
}

impl PassphraseSeparator {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Dash => "-",
            Self::Underscore => "_",
            Self::Space => " ",
            Self::Dot => ".",
        }
    }

    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Dash => "Dash (-)",
            Self::Underscore => "Underscore (_)",
            Self::Space => "Space ( )",
            Self::Dot => "Dot (.)",
        }
    }

    #[must_use]
    pub(crate) const fn all() -> &'static [Self] {
        &[Self::Dash, Self::Underscore, Self::Space, Self::Dot]
    }
}

/// Estimated cryptographic strength bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrengthLevel {
    Weak,
    Fair,
    Strong,
    VeryStrong,
}

impl StrengthLevel {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Weak => "Weak",
            Self::Fair => "Fair",
            Self::Strong => "Strong",
            Self::VeryStrong => "Very Strong",
        }
    }
}

/// Configuration options for generating passwords and passphrases.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PasswordConfig {
    pub(crate) mode: GeneratorMode,
    // Password options
    pub(crate) length: usize,
    pub(crate) uppercase: bool,
    pub(crate) lowercase: bool,
    pub(crate) numbers: bool,
    pub(crate) symbols: bool,
    pub(crate) exclude_ambiguous: bool,
    // Passphrase options
    pub(crate) word_count: usize,
    pub(crate) separator: PassphraseSeparator,
    pub(crate) capitalize: bool,
    pub(crate) include_number: bool,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            mode: GeneratorMode::Password,
            length: 20,
            uppercase: true,
            lowercase: true,
            numbers: true,
            symbols: true,
            exclude_ambiguous: false,
            word_count: 4,
            separator: PassphraseSeparator::Dash,
            capitalize: false,
            include_number: false,
        }
    }
}

/// UI state for the password generator panel.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub(crate) struct PasswordGeneratorState {
    pub(crate) config: PasswordConfig,
    pub(crate) current_value: String,
    pub(crate) reveal_value: bool,
    pub(crate) skip_history: bool,
}

impl Default for PasswordGeneratorState {
    fn default() -> Self {
        let config = PasswordConfig::default();
        let current_value = generate(&config);
        Self {
            config,
            current_value,
            reveal_value: true,
            skip_history: true,
        }
    }
}

impl PasswordGeneratorState {
    pub(crate) fn regenerate(&mut self) {
        self.current_value = generate(&self.config);
    }
}

const UPPER_ALL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const UPPER_UNAMBIGUOUS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
const LOWER_ALL: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const LOWER_UNAMBIGUOUS: &[u8] = b"abcdefghijkmnpqrstuvwxyz";
const DIGITS_ALL: &[u8] = b"0123456789";
const DIGITS_UNAMBIGUOUS: &[u8] = b"23456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{}|;:,.<>?";

/// 256 distinct, easy-to-type English words (8 bits of entropy per word).
const WORDLIST: &[&str] = &[
    "anchor",
    "apricot",
    "arctic",
    "arrow",
    "atlas",
    "autumn",
    "badge",
    "bamboo",
    "banjo",
    "beacon",
    "beetle",
    "bison",
    "blazer",
    "blossom",
    "boulder",
    "breeze",
    "bridge",
    "bronze",
    "brook",
    "bucket",
    "cactus",
    "camera",
    "candle",
    "canyon",
    "carpet",
    "castle",
    "cedar",
    "celery",
    "chalk",
    "cherry",
    "chimney",
    "cinder",
    "circle",
    "citadel",
    "clamor",
    "clover",
    "cobalt",
    "cobble",
    "coffee",
    "comet",
    "compass",
    "copper",
    "coral",
    "cotton",
    "cougar",
    "cradle",
    "crayon",
    "cricket",
    "crimson",
    "crystal",
    "cypress",
    "dagger",
    "daisy",
    "dancer",
    "dawn",
    "delta",
    "desert",
    "diamond",
    "dolphin",
    "domino",
    "dragon",
    "drizzle",
    "drum",
    "dune",
    "eagle",
    "echo",
    "eclipse",
    "ember",
    "emerald",
    "engine",
    "epoch",
    "falcon",
    "feather",
    "fennel",
    "fern",
    "fiddle",
    "filter",
    "finch",
    "fjord",
    "flame",
    "flannel",
    "flint",
    "flower",
    "flute",
    "forest",
    "fossil",
    "fountain",
    "fox",
    "frost",
    "galaxy",
    "garden",
    "garlic",
    "garnet",
    "geyser",
    "ginger",
    "glacier",
    "glider",
    "globe",
    "granite",
    "grape",
    "gravel",
    "griffin",
    "grove",
    "guitar",
    "harbor",
    "harvest",
    "haven",
    "hazel",
    "helmet",
    "heron",
    "hickory",
    "horizon",
    "hornet",
    "icicle",
    "iguana",
    "indigo",
    "island",
    "ivory",
    "jacket",
    "jaguar",
    "jasper",
    "jigsaw",
    "juniper",
    "kayak",
    "kettle",
    "kestrel",
    "kingdom",
    "kite",
    "koala",
    "lagoon",
    "lantern",
    "larch",
    "laurel",
    "lavender",
    "lemon",
    "leopard",
    "lichen",
    "lighthouse",
    "lilac",
    "linden",
    "lizard",
    "lotus",
    "lumber",
    "lunar",
    "magnet",
    "magpie",
    "mango",
    "maple",
    "marble",
    "marina",
    "marmot",
    "meadow",
    "melon",
    "mercury",
    "meteor",
    "mineral",
    "mint",
    "mirror",
    "monarch",
    "monkey",
    "monsoon",
    "moose",
    "mosaic",
    "moss",
    "muffin",
    "mulberry",
    "mustard",
    "myrtle",
    "narwhal",
    "nectar",
    "needle",
    "neptune",
    "nickel",
    "nimbus",
    "nutmeg",
    "oasis",
    "obsidian",
    "ocean",
    "octane",
    "olive",
    "onyx",
    "opal",
    "orange",
    "orchid",
    "oriole",
    "osprey",
    "otter",
    "owl",
    "oxygen",
    "paddle",
    "palace",
    "panda",
    "panther",
    "papaya",
    "parchment",
    "parrot",
    "pebble",
    "pelican",
    "pepper",
    "perch",
    "petal",
    "pewter",
    "phoenix",
    "picnic",
    "pigeon",
    "pillow",
    "pine",
    "pixel",
    "planet",
    "plover",
    "plum",
    "pocket",
    "polar",
    "poplar",
    "porch",
    "prairie",
    "prism",
    "puffin",
    "pumpkin",
    "puzzle",
    "python",
    "quartz",
    "quiver",
    "rabbit",
    "radish",
    "rainbow",
    "rapids",
    "raven",
    "redwood",
    "reef",
    "ribbon",
    "ridge",
    "ripple",
    "river",
    "robin",
    "rocket",
    "rooster",
    "rose",
    "ruby",
    "saddle",
    "saffron",
    "salmon",
    "sandal",
    "sapphire",
    "saturn",
    "scarlet",
    "schooner",
    "scooter",
    "sequoia",
    "shadow",
    "shell",
    "shingle",
    "sierra",
    "silver",
    "skipper",
    "skylark",
];

fn active_char_pools(config: &PasswordConfig) -> Vec<&'static [u8]> {
    let mut pools = Vec::with_capacity(4);
    if config.uppercase {
        pools.push(if config.exclude_ambiguous {
            UPPER_UNAMBIGUOUS
        } else {
            UPPER_ALL
        });
    }
    if config.lowercase {
        pools.push(if config.exclude_ambiguous {
            LOWER_UNAMBIGUOUS
        } else {
            LOWER_ALL
        });
    }
    if config.numbers {
        pools.push(if config.exclude_ambiguous {
            DIGITS_UNAMBIGUOUS
        } else {
            DIGITS_ALL
        });
    }
    if config.symbols {
        pools.push(SYMBOLS);
    }
    if pools.is_empty() {
        pools.push(if config.exclude_ambiguous {
            LOWER_UNAMBIGUOUS
        } else {
            LOWER_ALL
        });
    }
    pools
}

/// Generates a random password or passphrase according to `config`.
#[must_use]
pub(crate) fn generate(config: &PasswordConfig) -> String {
    match config.mode {
        GeneratorMode::Password => generate_password(config),
        GeneratorMode::Passphrase => generate_passphrase(config),
    }
}

/// Generates a cryptographically random password guaranteeing at least one character from each
/// enabled character class whenever `length >= enabled_classes`.
#[must_use]
pub(crate) fn generate_password(config: &PasswordConfig) -> String {
    let length = config.length.clamp(4, 128);
    let pools = active_char_pools(config);
    let combined: Vec<u8> = pools.iter().flat_map(|p| p.iter().copied()).collect();

    let mut rng = rand::rng();
    let mut chars: Vec<u8> = Vec::with_capacity(length);

    // Guarantee at least one character from each active class
    for pool in &pools {
        if chars.len() < length {
            let idx = rng.random_range(0..pool.len());
            chars.push(pool[idx]);
        }
    }

    while chars.len() < length {
        let idx = rng.random_range(0..combined.len());
        chars.push(combined[idx]);
    }

    chars.shuffle(&mut rng);
    String::from_utf8(chars).unwrap_or_default()
}

/// Generates a random Diceware-style passphrase from the built-in wordlist.
#[must_use]
pub(crate) fn generate_passphrase(config: &PasswordConfig) -> String {
    let word_count = config.word_count.clamp(2, 12);
    let mut rng = rand::rng();
    let number_word_idx = if config.include_number {
        Some(rng.random_range(0..word_count))
    } else {
        None
    };

    let mut words = Vec::with_capacity(word_count);
    for i in 0..word_count {
        let idx = rng.random_range(0..WORDLIST.len());
        let raw = WORDLIST[idx];
        let mut word = if config.capitalize {
            let mut c = raw.chars();
            c.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + c.as_str()
            })
        } else {
            raw.to_string()
        };

        if number_word_idx == Some(i) {
            let n = rng.random_range(10..100);
            word.push_str(&n.to_string());
        }
        words.push(word);
    }

    words.join(config.separator.as_str())
}

/// Estimates combinatorial entropy in bits for the given configuration.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(crate) fn estimate_entropy_bits(config: &PasswordConfig) -> f64 {
    match config.mode {
        GeneratorMode::Password => {
            let pool_size: usize = active_char_pools(config).iter().map(|p| p.len()).sum();
            if pool_size <= 1 {
                return 0.0;
            }
            (config.length as f64) * (pool_size as f64).log2()
        }
        GeneratorMode::Passphrase => {
            let base = (config.word_count as f64) * (WORDLIST.len() as f64).log2();
            if config.include_number {
                base + 90.0_f64.log2()
            } else {
                base
            }
        }
    }
}

/// Classifies an entropy estimate (in bits) into a human-readable strength bucket.
#[must_use]
pub(crate) fn classify_strength(entropy_bits: f64) -> StrengthLevel {
    if entropy_bits < 50.0 {
        StrengthLevel::Weak
    } else if entropy_bits < 70.0 {
        StrengthLevel::Fair
    } else if entropy_bits < 100.0 {
        StrengthLevel::Strong
    } else {
        StrengthLevel::VeryStrong
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wordlist_has_256_unique_words() {
        assert_eq!(WORDLIST.len(), 256);
        let unique: std::collections::HashSet<&str> = WORDLIST.iter().copied().collect();
        assert_eq!(unique.len(), 256);
    }

    #[test]
    fn test_password_respects_length_and_classes() {
        let cfg = PasswordConfig {
            length: 24,
            uppercase: true,
            lowercase: true,
            numbers: true,
            symbols: true,
            ..PasswordConfig::default()
        };
        let pw = generate_password(&cfg);
        assert_eq!(pw.len(), 24);
        assert!(pw.chars().any(|c| c.is_ascii_uppercase()));
        assert!(pw.chars().any(|c| c.is_ascii_lowercase()));
        assert!(pw.chars().any(|c| c.is_ascii_digit()));
        assert!(pw.chars().any(|c| !c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_password_excludes_ambiguous_characters() {
        let cfg = PasswordConfig {
            length: 64,
            uppercase: true,
            lowercase: true,
            numbers: true,
            symbols: false,
            exclude_ambiguous: true,
            ..PasswordConfig::default()
        };
        for _ in 0..10 {
            let pw = generate_password(&cfg);
            assert!(!pw.contains(['0', 'O', 'o', '1', 'l', 'I']));
        }
    }

    #[test]
    fn test_password_fallback_when_all_classes_disabled() {
        let cfg = PasswordConfig {
            length: 12,
            uppercase: false,
            lowercase: false,
            numbers: false,
            symbols: false,
            ..PasswordConfig::default()
        };
        let pw = generate_password(&cfg);
        assert_eq!(pw.len(), 12);
        assert!(pw.chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn test_passphrase_word_count_separator_and_options() {
        let cfg = PasswordConfig {
            mode: GeneratorMode::Passphrase,
            word_count: 5,
            separator: PassphraseSeparator::Underscore,
            capitalize: true,
            include_number: true,
            ..PasswordConfig::default()
        };
        let phrase = generate_passphrase(&cfg);
        let parts: Vec<&str> = phrase.split('_').collect();
        assert_eq!(parts.len(), 5);
        assert!(parts
            .iter()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase)));
        assert!(phrase.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_entropy_and_strength_classification() {
        let weak = PasswordConfig {
            mode: GeneratorMode::Password,
            length: 8,
            uppercase: false,
            lowercase: true,
            numbers: false,
            symbols: false,
            ..PasswordConfig::default()
        };
        let weak_bits = estimate_entropy_bits(&weak);
        assert!(weak_bits < 50.0);
        assert_eq!(classify_strength(weak_bits), StrengthLevel::Weak);

        let strong = PasswordConfig::default(); // 20 chars, full pool
        let strong_bits = estimate_entropy_bits(&strong);
        assert!(strong_bits >= 100.0);
        assert_eq!(classify_strength(strong_bits), StrengthLevel::VeryStrong);
    }
}
