//! Reads app types (`common.type`: Game, Tool, Application, Demo...) from Steam's binary `appcache/appinfo.vdf`.
//! Only the apps asked for are decoded; every other record is skipped by its size.

use std::collections::{HashMap, HashSet};

const V27: u32 = 0x0756_4427;
const V28: u32 = 0x0756_4428;
const V29: u32 = 0x0756_4429;

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn cstr(&mut self) -> Option<String> {
        let rest = self.b.get(self.pos..)?;
        let end = rest.iter().position(|&c| c == 0)?;
        self.pos += end + 1;
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
    fn wstr(&mut self) -> Option<()> {
        while self.take(2)? != [0, 0] {}
        Some(())
    }
}

/// Binary KeyValues: v29 stores keys as indexes into a string table at the end of the file.
struct Kv<'a> {
    strings: Option<&'a [String]>,
}

impl Kv<'_> {
    fn key(&self, r: &mut Reader) -> Option<String> {
        match self.strings {
            Some(t) => t.get(r.u32()? as usize).cloned(),
            None => r.cstr(),
        }
    }

    /// Walks one map; returns `common.type` if met on the way. `path` is the chain of parent keys.
    fn find_type(&self, r: &mut Reader, path: &mut Vec<String>) -> Option<Option<String>> {
        let mut found = None;
        loop {
            let t = r.u8()?;
            if t == 0x08 || t == 0x0B {
                return Some(found);
            }
            let key = self.key(r)?;
            match t {
                0x00 => {
                    path.push(key);
                    let inner = self.find_type(r, path)?;
                    path.pop();
                    found = found.or(inner);
                }
                0x01 => {
                    let v = r.cstr()?;
                    let in_common = path.last().is_some_and(|p| p.eq_ignore_ascii_case("common"));
                    if found.is_none() && in_common && key.eq_ignore_ascii_case("type") {
                        found = Some(v);
                    }
                }
                0x02 | 0x03 | 0x04 | 0x06 => {
                    r.take(4)?;
                }
                0x05 => r.wstr()?,
                0x07 | 0x0A => {
                    r.take(8)?;
                }
                _ => return None,
            }
        }
    }
}

fn string_table(b: &[u8], offset: usize) -> Option<Vec<String>> {
    let mut r = Reader { b, pos: offset };
    let n = r.u32()? as usize;
    let mut out = Vec::with_capacity(n.min(1 << 20));
    for _ in 0..n {
        out.push(r.cstr()?);
    }
    Some(out)
}

/// appid -> lowercased `common.type` for the `wanted` apps found in `appinfo.vdf` bytes.
pub fn app_types(b: &[u8], wanted: &HashSet<u32>) -> Option<HashMap<u32, String>> {
    let mut r = Reader { b, pos: 0 };
    let magic = r.u32()?;
    if ![V27, V28, V29].contains(&magic) {
        return None;
    }
    r.u32()?; // universe
    let (strings, end) = if magic == V29 {
        let off = r.u64()? as usize;
        (Some(string_table(b, off)?), off)
    } else {
        (None, b.len())
    };
    let kv = Kv { strings: strings.as_deref() };
    // info_state, last_updated, pics token, text sha1, change number (+ binary sha1 since v28).
    let head = if magic == V27 { 40 } else { 60 };
    let mut out = HashMap::new();
    while r.pos + 8 <= end {
        let appid = r.u32()?;
        if appid == 0 {
            break;
        }
        let size = r.u32()? as usize;
        let next = r.pos.checked_add(size)?;
        if wanted.contains(&appid) && size > head {
            let mut body = Reader { b: b.get(..next)?, pos: r.pos + head };
            if let Some(Some(t)) = kv.find_type(&mut body, &mut Vec::new()) {
                out.insert(appid, t.to_lowercase());
            }
        }
        r.pos = next;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One v29 record: appinfo { appid 7, common { name "X", type "Tool" } }.
    fn sample() -> Vec<u8> {
        let strings = ["appinfo", "appid", "common", "name", "type"];
        let mut kv = vec![0x00];
        kv.extend(0u32.to_le_bytes());
        kv.push(0x02);
        kv.extend(1u32.to_le_bytes());
        kv.extend(7u32.to_le_bytes());
        kv.push(0x00);
        kv.extend(2u32.to_le_bytes());
        kv.push(0x01);
        kv.extend(3u32.to_le_bytes());
        kv.extend(b"X\0");
        kv.push(0x01);
        kv.extend(4u32.to_le_bytes());
        kv.extend(b"Tool\0");
        kv.extend([0x08, 0x08, 0x08]);
        let mut rec = vec![0u8; 60];
        rec.extend(&kv);
        let mut b = Vec::new();
        b.extend(V29.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        let table_at = 16 + 8 + rec.len() + 4;
        b.extend((table_at as u64).to_le_bytes());
        b.extend(7u32.to_le_bytes());
        b.extend((rec.len() as u32).to_le_bytes());
        b.extend(&rec);
        b.extend(0u32.to_le_bytes());
        b.extend((strings.len() as u32).to_le_bytes());
        for s in strings {
            b.extend(s.as_bytes());
            b.push(0);
        }
        b
    }

    #[test]
    fn reads_v29_type() {
        let t = app_types(&sample(), &HashSet::from([7])).unwrap();
        assert_eq!(t.get(&7).map(String::as_str), Some("tool"));
        assert!(app_types(&sample(), &HashSet::from([8])).unwrap().is_empty());
    }
}
