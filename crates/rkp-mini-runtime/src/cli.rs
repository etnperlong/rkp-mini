//! Option parsing shared by both command parsers (hand-written, no clap).

use anyhow::{bail, Result};

/// A confirmation digest: lowercase hex, 16..=64 chars.
pub fn sha_arg(v: String) -> Result<String> {
    let v = v.to_ascii_lowercase();
    if !(16..=64).contains(&v.len()) || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("confirmation values are sha256 hex (full or a prefix of at least 16 chars)");
    }
    Ok(v)
}

/// Remaining command arguments, consumed by name.
pub struct Flags {
    args: Vec<String>,
}

impl Flags {
    pub fn new(args: Vec<String>) -> Self {
        Flags { args }
    }

    pub fn flag(&mut self, name: &str) -> bool {
        match self.args.iter().position(|a| a == name) {
            Some(i) => {
                self.args.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn value(&mut self, name: &str) -> Result<Option<String>> {
        let Some(i) = self.args.iter().position(|a| a == name) else {
            return Ok(None);
        };
        if i + 1 >= self.args.len() || self.args[i + 1].starts_with("--") {
            bail!("{name} needs a value");
        }
        let v = self.args.remove(i + 1);
        self.args.remove(i);
        Ok(Some(v))
    }

    pub fn positional(&mut self) -> Option<String> {
        let i = self.args.iter().position(|a| !a.starts_with("--"))?;
        Some(self.args.remove(i))
    }

    /// Next remaining argument, even if it looks like a flag (config values).
    pub fn positional_any(&mut self) -> Option<String> {
        (!self.args.is_empty()).then(|| self.args.remove(0))
    }

    pub fn finish(self) -> Result<()> {
        if let Some(a) = self.args.first() {
            bail!("unexpected argument `{a}`");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_are_lowercased_and_length_checked() {
        assert_eq!(
            sha_arg("ABCDEF0123456789abcdef".into()).unwrap(),
            "abcdef0123456789abcdef"
        );
        assert!(sha_arg("abcdef".into()).is_err(), "prefix too short");
        assert!(sha_arg("xyz0123456789abcdef".into()).is_err());
    }

    #[test]
    fn flags_consume_by_name() {
        let mut f = Flags::new(
            ["--target", "omk", "--target", "teesim", "--yes"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        assert_eq!(f.value("--target").unwrap().as_deref(), Some("omk"));
        assert_eq!(f.value("--target").unwrap().as_deref(), Some("teesim"));
        assert!(f.flag("--yes"));
        assert!(f.value("--target").unwrap().is_none());
        f.finish().unwrap();
        let g = Flags::new(vec!["extra".into()]);
        assert!(g.finish().is_err());
    }
}
