use lazy_regex::{Regex, RegexBuilder, regex};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Always returns `true`.
///
/// Useful for default values in rule configs that use serde.
/// See [serde documentation](https://serde.rs/field-attrs.html#default--path)
/// for more information
///
/// ## Example
/// ```ignore
/// use serde::Deserialize;
/// use oxc_linter::utils::default_true;
///
/// #[derive(Debug, Clone, Deserialize)]
/// pub struct RuleConfig {
///     // default to true
///     #[serde(default = "default_true")]
///     pub foo: bool,
///     // default to false
///     #[serde(default)]
///     pub bar: bool,
/// }
/// ```
#[inline]
pub const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlwaysNever {
    #[default]
    Always,
    Never,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AllowedOrDisallowInFunc {
    #[default]
    Allowed,
    DisallowInFunc,
}

pub fn deserialize_regex_option<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    Option::<String>::deserialize(deserializer)?
        .map(|pattern| RegexBuilder::new(&pattern).build())
        .transpose()
        .map_err(D::Error::custom)
}

pub fn deserialize_regex<'de, D>(deserializer: D) -> Result<Regex, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let pattern = String::deserialize(deserializer)?;
    RegexBuilder::new(&pattern).build().map_err(D::Error::custom)
}

pub fn deserialize_regex_vec<'de, D>(deserializer: D) -> Result<Vec<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|pattern| RegexBuilder::new(&pattern).build())
        .collect::<Result<Vec<_>, _>>()
        .map_err(D::Error::custom)
}

pub fn deserialize_required_regex_option<'de, D>(deserializer: D) -> Result<Option<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let pattern = String::deserialize(deserializer)?;
    RegexBuilder::new(&pattern).build().map(Some).map_err(D::Error::custom)
}

/// Compiles a single pattern string using eslint-plugin-vue's
/// `utils/regexp.js` `toRegExp` semantics, used by config options like
/// `ignoreTags` that are matched via `toRegExpGroupMatcher`:
/// - A bare string (e.g. `"MyTag"`) becomes an **escaped, anchored**
///   exact-match regex (`^MyTag$`) — NOT a substring search. This differs
///   from [`deserialize_regex`]/[`deserialize_regex_vec`], which compile
///   their pattern strings as regexes directly (unanchored); this function
///   exists specifically for options that mirror `toRegExp`'s dual string
///   handling.
/// - A `"/pattern/flags"` string (matching `/^\/(.+)\/(.*)$/`) is parsed as
///   a real regex: `pattern` is used as the regex source, and any of the
///   `i` (case-insensitive), `m` (multi-line `^`/`$`), and `s` (`.` matches
///   newlines, JS's `dotAll`) flags are honored via the matching
///   `RegexBuilder` option. JS's `g` (global) and `y` (sticky) flags have no
///   compile-time equivalent for a single `.is_match()` check (JS itself
///   only uses them for stateful iteration) and are silently ignored, same
///   as upstream's own `toRegExp(pattern, { remove: "g" })` callers
///   stripping `g` before compiling. `u` (unicode) is a no-op here: the
///   `regex` crate matches on Unicode scalar values unconditionally, unlike
///   JS where it's opt-in.
pub fn to_regexp(pattern: &str) -> Result<Regex, regex::Error> {
    if let Some((body, flags)) = split_regexp_literal(pattern) {
        let mut builder = RegexBuilder::new(body);
        for flag in flags.chars() {
            match flag {
                'i' => {
                    builder.case_insensitive(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                // `g`/`y`/`u`/`d`: no match-time equivalent (see doc comment
                // above); silently ignored.
                _ => {}
            }
        }
        builder.build()
    } else {
        RegexBuilder::new(&format!("^{}$", regex::escape(pattern))).build()
    }
}

/// Splits a `"/pattern/flags"` string into `(pattern, flags)`, mirroring
/// eslint-plugin-vue's `RE_REGEXP_STR = /^\/(.+)\/(.*)$/u`: the pattern body
/// must be non-empty. Returns `None` for anything else (including a bare
/// leading `/` with no closing `/`, or an empty `//flags` body), in which
/// case the whole original string is treated as a literal by the caller.
fn split_regexp_literal(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('/')?;
    let slash_index = rest.rfind('/')?;
    if slash_index == 0 {
        return None;
    }
    Some((&rest[..slash_index], &rest[slash_index + 1..]))
}

/// Deserializes a list of pattern strings via [`to_regexp`], for config
/// options (like `ignoreTags`) that use eslint-plugin-vue's
/// `toRegExpGroupMatcher` semantics.
pub fn deserialize_to_regexp_group_vec<'de, D>(deserializer: D) -> Result<Vec<Regex>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|pattern| to_regexp(&pattern))
        .collect::<Result<Vec<_>, _>>()
        .map_err(D::Error::custom)
}
