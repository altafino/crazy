//! Release versions (M4-D8): strict `MAJOR.MINOR.PATCH[-prerelease]` semver, no `+build`.
//! A validated version is used as a directory name and passed to `--config`, so the parser is
//! deliberately narrow: digits, ASCII letters, `.` and `-` only, no leading zeros.

use std::cmp::Ordering;

const MAX_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// Dot-separated identifiers after `-`.
    pre: Vec<String>,
}

fn number(s: &str) -> Option<u64> {
    let ok =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if ok { s.parse().ok() } else { None }
}

impl Version {
    /// `None` unless `s` is a valid version.
    pub(crate) fn parse(s: &str) -> Option<Self> {
        if s.is_empty() || s.len() > MAX_LEN {
            return None;
        }
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut nums = core.split('.');
        let (major, minor, patch) = (
            number(nums.next()?)?,
            number(nums.next()?)?,
            number(nums.next()?)?,
        );
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_owned).collect();
                let ok = ids.iter().all(|id| {
                    !id.is_empty()
                        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        // A numeric identifier has no leading zero.
                        && (!id.bytes().all(|b| b.is_ascii_digit()) || number(id).is_some())
                });
                if !ok {
                    return None;
                }
                ids
            }
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// The next release above this one: a prerelease's own core, else the next patch.
    pub(crate) fn next(&self) -> String {
        if self.pre.is_empty() {
            format!("{}.{}.{}", self.major, self.minor, self.patch + 1)
        } else {
            format!("{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

fn cmp_ident(a: &str, b: &str) -> Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = cmp_ident(a, b);
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// The suggested version for the next build: the project's configured version when it is
/// above every release, else the next one above the highest release; "0.1.0" for a first
/// build without a usable configured version.
pub(crate) fn suggest<'a>(
    released: impl IntoIterator<Item = &'a str>,
    configured: Option<&str>,
) -> String {
    let highest = released.into_iter().filter_map(Version::parse).max();
    let configured = configured.filter(|c| Version::parse(c).is_some());
    match (highest, configured) {
        (None, None) => "0.1.0".to_owned(),
        (None, Some(c)) => c.to_owned(),
        (Some(h), Some(c)) if Version::parse(c).is_some_and(|c| c > h) => c.to_owned(),
        (Some(h), _) => h.next(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_strict_semver_and_prereleases() {
        for v in [
            "0.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-rc.1",
            "1.0.0-alpha-2",
            "1.0.0-0",
        ] {
            assert!(Version::parse(v).is_some(), "{v}");
        }
    }

    #[test]
    fn rejects_everything_else() {
        for v in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "v1.2.3",
            "1.2.3+build",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-01",
            "1.2.3-a_b",
            "../1.2.3",
            "1.2.3/x",
            "1.2.3 ",
            "-1.2.3",
            "1.2.-3",
            "99999999999999999999.0.0",
        ] {
            assert!(Version::parse(v).is_none(), "{v:?}");
        }
        assert!(Version::parse(&format!("1.0.0-{}", "a".repeat(70))).is_none());
    }

    #[test]
    fn orders_like_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"));
        assert!(v("1.0.0-alpha.beta") < v("1.0.0-beta"));
        assert!(v("1.0.0-rc.2") < v("1.0.0-rc.10"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.9.0") < v("1.10.0"));
    }

    #[test]
    fn suggests_the_next_version() {
        assert_eq!(suggest([], None), "0.1.0");
        assert_eq!(suggest([], Some("0.3.0")), "0.3.0");
        assert_eq!(suggest([], Some("garbage")), "0.1.0");
        assert_eq!(suggest(["0.1.0"], Some("0.1.0")), "0.1.1");
        assert_eq!(suggest(["0.1.0", "0.1.9", "0.1.10"], None), "0.1.11");
        assert_eq!(suggest(["1.0.5"], Some("2.0.0")), "2.0.0");
        assert_eq!(suggest(["1.0.5"], Some("1.0.0")), "1.0.6");
        assert_eq!(suggest(["1.0.0-beta.1"], None), "1.0.0");
        assert_eq!(suggest(["junk", "1.0.0"], None), "1.0.1");
    }
}
