use std::fmt;

/// A dotted numeric version such as `1.45` or `2.24.4`, compared component by component.
#[derive(Debug, Clone)]
pub struct Version(Vec<u64>);

impl Version {
    /// Reads the leading numeric components, ignoring a `v` prefix and any suffix
    /// such as `-desktop.1`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = Vec::new();
        for part in text.split('.') {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                break;
            }
            parts.push(digits.parse().ok()?);
            if digits.len() != part.len() {
                break;
            }
        }
        (!parts.is_empty()).then_some(Self(parts))
    }

    fn component(&self, index: usize) -> u64 {
        self.0.get(index).copied().unwrap_or(0)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let len = self.0.len().max(other.0.len());
        (0..len)
            .map(|i| self.component(i).cmp(&other.component(i)))
            .find(|ordering| ordering.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_prefixes_and_suffixes() {
        assert_eq!(v("v2.24.4"), v("2.24.4"));
        assert_eq!(v("2.29.1-desktop.1"), v("2.29.1"));
        assert_eq!(v("1.45"), v("1.45.0"));
        assert_eq!(Version::parse("desktop"), None);
    }

    #[test]
    fn compares_numerically() {
        assert!(v("1.44") < v("1.45"));
        assert!(v("2.24.3") < v("2.24.4"));
        assert!(v("2.100.0") > v("2.24.4"));
        assert!(v("5.7.1") > v("2.24.4"));
    }
}
