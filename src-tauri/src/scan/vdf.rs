//! Minimal Valve KeyValues (text VDF) reader: enough for libraryfolders.vdf and appmanifest_*.acf.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Vdf {
    Str(String),
    Map(BTreeMap<String, Vdf>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Vdf)> {
        let m = match self {
            Vdf::Map(m) => Some(m),
            Vdf::Str(_) => None,
        };
        m.into_iter().flat_map(|m| m.iter())
    }
}

enum Tok {
    Str(String),
    Open,
    Close,
}

fn tokens(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = src.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' => out.push(Tok::Open),
            '}' => out.push(Tok::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '\\' => match it.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        '"' => break,
                        o => s.push(o),
                    }
                }
                out.push(Tok::Str(s));
            }
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                // Unquoted token (rare in Steam files).
                let mut s = String::from(c);
                while let Some(&n) = it.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    it.next();
                }
                out.push(Tok::Str(s));
            }
        }
    }
    out
}

fn map(toks: &mut std::iter::Peekable<std::vec::IntoIter<Tok>>) -> BTreeMap<String, Vdf> {
    let mut m = BTreeMap::new();
    while let Some(t) = toks.next() {
        let key = match t {
            Tok::Str(k) => k,
            Tok::Close => break,
            Tok::Open => continue,
        };
        match toks.next() {
            Some(Tok::Str(v)) => {
                m.insert(key, Vdf::Str(v));
            }
            Some(Tok::Open) => {
                m.insert(key, Vdf::Map(map(toks)));
            }
            _ => break,
        }
    }
    m
}

pub fn parse(src: &str) -> Vdf {
    let mut toks = tokens(src).into_iter().peekable();
    Vdf::Map(map(&mut toks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_appmanifest() {
        let v = parse(r#""AppState" { "appid" "271590" "name" "Grand Theft Auto V" "buildid" "123" "UserConfig" { "language" "english" } }"#);
        let s = v.get("AppState").unwrap();
        assert_eq!(s.str("appid"), Some("271590"));
        assert_eq!(s.str("NAME"), Some("Grand Theft Auto V"));
        assert_eq!(s.get("UserConfig").unwrap().str("language"), Some("english"));
    }
}
