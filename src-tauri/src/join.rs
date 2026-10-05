//! Multiplayer joins (docs/RECIPE-FORMAT.md section 9): invite links, join addresses and the extra launch arguments
//! that take a player straight into a lobby's game. Pure functions here; the commands that fetch, install and launch
//! are in lib.rs.

use crate::install::Strategy;
use serde::{Deserialize, Serialize};

/// Where the app talks to the lobby API.
pub const SITE: &str = "https://sigf.ai";

/// Engines that join a server with `+connect <addr>` on the command line (Source, GoldSrc, Quake). Kept in step with
/// the sigf.ai lobby API's list (docs/RECIPE-FORMAT.md section 9.4).
pub const CONNECT_GAMES: &[&str] = &["tf2", "gmod", "portal2", "cs16", "css", "hl2dm", "l4d2", "quake"];

/// A lobby id: 12 characters of `[a-km-z2-9]` (shared/lobbies.ts `LOBBY_ID_RE`).
pub fn valid_lobby_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| matches!(b, b'a'..=b'k' | b'm'..=b'z' | b'2'..=b'9'))
}

/// The lobby id of an invite: `sigf://join/<id>`, `https://sigf.ai/join/<id>` (what gets pasted), or the bare id.
/// A trailing `/`, a query or a fragment is ignored; anything else is not an invite.
pub fn parse_link(link: &str) -> Option<String> {
    let s = link.trim();
    let rest = if let Some(r) = strip_prefix_ci(s, "sigf://join/") {
        r
    } else if let Some(r) = strip_prefix_ci(s, "https://sigf.ai/join/").or_else(|| strip_prefix_ci(s, "https://www.sigf.ai/join/")) {
        r
    } else {
        s
    };
    let id = rest.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    valid_lobby_id(id).then(|| id.to_string())
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

/// `host:port` as the API accepts it (shared/lobbies.ts `parseAddress`): a DNS name, an IPv4 or a bracketed IPv6,
/// and a port 1..65535. It goes onto a game's command line, so nothing that could read as an option or a second arg.
pub fn valid_address(s: &str) -> bool {
    if s.is_empty() || s.len() > 260 {
        return false;
    }
    let Some((host, port)) = s.rsplit_once(':') else { return false };
    let port_ok = !port.is_empty() && port.len() <= 5 && !port.starts_with('0') && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u32>().map(|p| (1..=65535).contains(&p)).unwrap_or(false);
    if !port_ok {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return (2..=45).contains(&inner.len()) && inner.bytes().all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.');
    }
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|l| {
        !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// How a player joins on one game, from the installed strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JoinKind {
    /// Minecraft through Prism: `--launch <instance> --server <addr>`.
    Prism,
    /// `+connect <addr>` after the recipe's own args.
    Connect,
    /// The mod's own networking: the address is written to `{app}/sigf-join.json`, the launch is unchanged.
    Mod,
}

pub fn join_kind(strategy: Strategy, game: &str) -> JoinKind {
    if strategy == Strategy::Mrpack {
        JoinKind::Prism
    } else if CONNECT_GAMES.contains(&game) {
        JoinKind::Connect
    } else {
        JoinKind::Mod
    }
}

/// Prism's arguments to start an instance, joined to `address` when there is one.
pub fn prism_args(instance: &str, address: Option<&str>) -> Vec<String> {
    let mut a = vec!["--launch".to_string(), instance.to_string()];
    if let Some(addr) = address {
        a.push("--server".into());
        a.push(addr.into());
    }
    a
}

/// The launch args of a non-Minecraft game for a join: the recipe's args, plus `+connect <addr>` on a connect engine.
pub fn store_join_args(kind: JoinKind, base: &[String], address: Option<&str>) -> Vec<String> {
    let mut a = base.to_vec();
    if let (JoinKind::Connect, Some(addr)) = (kind, address) {
        a.push("+connect".into());
        a.push(addr.into());
    }
    a
}

/// `{app}/sigf-join.json` for a `mod` join: what the mod's own networking reads.
pub fn join_file(lobby: &str, address: &str, host: &str) -> String {
    serde_json::json!({ "lobby": lobby, "address": address, "host": host }).to_string()
}

/// Minecraft's protocol VarInt.
pub fn varint(v: i32) -> Vec<u8> {
    let mut u = v as u32;
    let mut out = vec![];
    loop {
        if u & !0x7f == 0 {
            out.push(u as u8);
            return out;
        }
        out.push((u & 0x7f | 0x80) as u8);
        u >>= 7;
    }
}

pub fn read_varint(r: &mut impl std::io::Read) -> std::io::Result<i32> {
    let mut v: u32 = 0;
    for i in 0..5 {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        v |= ((b[0] & 0x7f) as u32) << (7 * i);
        if b[0] & 0x80 == 0 {
            return Ok(v as i32);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "varint too long"))
}

fn packet(body: &[u8]) -> Vec<u8> {
    let mut p = varint(body.len() as i32);
    p.extend_from_slice(body);
    p
}

/// Server List Ping: the handshake (next state: status) then the status request, as one write.
pub fn status_request(host: &str, port: u16) -> Vec<u8> {
    let mut hs = vec![0x00];
    hs.extend(varint(-1));
    hs.extend(varint(host.len() as i32));
    hs.extend_from_slice(host.as_bytes());
    hs.extend_from_slice(&port.to_be_bytes());
    hs.extend(varint(1));
    let mut out = packet(&hs);
    out.extend(packet(&[0x00]));
    out
}

/// `players.online` / `players.max` of a status answer's JSON.
pub fn parse_status(json: &str) -> Option<(u32, u32)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let p = v.get("players")?;
    Some((p.get("online")?.as_u64()? as u32, p.get("max")?.as_u64()? as u32))
}

/// Players on a Minecraft server (the host's own world, for the lobby's live count), or None if it does not answer
/// within 2 s. Read-only: the status ping never joins the game.
pub fn minecraft_players(address: &str) -> Option<(u32, u32)> {
    use std::io::{Read, Write};
    use std::net::ToSocketAddrs;
    if !valid_address(address) {
        return None;
    }
    let (host, port) = address.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let sock = address.to_socket_addrs().ok()?.next()?;
    let t = std::time::Duration::from_secs(2);
    let mut s = std::net::TcpStream::connect_timeout(&sock, t).ok()?;
    s.set_read_timeout(Some(t)).ok()?;
    s.write_all(&status_request(host.trim_start_matches('[').trim_end_matches(']'), port)).ok()?;
    let _len = read_varint(&mut s).ok()?;
    if read_varint(&mut s).ok()? != 0 {
        return None;
    }
    let n = read_varint(&mut s).ok()?;
    if !(0..=262_144).contains(&n) {
        return None;
    }
    let mut buf = vec![0u8; n as usize];
    s.read_exact(&mut buf).ok()?;
    parse_status(&String::from_utf8_lossy(&buf))
}

/// One target of a lobby as the API returns it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
    pub game: String,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LobbyMashup {
    pub id: String,
    pub version: String,
    pub name: String,
}

/// What the join path needs from `GET /api/app/lobbies/<id>`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lobby {
    pub id: String,
    pub mashup: LobbyMashup,
    pub host: String,
    pub state: String,
    #[serde(default)]
    pub targets: Vec<Target>,
}

/// What the player confirmed in the join sheet (mashup, version, game -> server address). The join is refused when the
/// lobby no longer matches it, so a host cannot change the address between the question and the install.
#[derive(Debug, Clone, Deserialize)]
pub struct Confirmed {
    pub mashup: String,
    pub version: String,
    #[serde(default)]
    pub targets: std::collections::HashMap<String, String>,
}

impl Confirmed {
    pub fn matches(&self, l: &Lobby) -> bool {
        self.mashup == l.mashup.id
            && self.version == l.mashup.version
            && self.targets.len() == l.targets.len()
            && l.targets.iter().all(|t| self.targets.get(&t.game) == Some(&t.address))
    }
}

/// The join's error, typed for the UI: `lobbyClosed`, `lobbyNotFound`, `lobbyFull`, `notReady`, `network`,
/// `badLobby`, `launch`, or an install error's own kind (`needsLauncher`, `shaMismatch`, ...).
#[derive(Debug, Clone, Serialize)]
pub struct JoinError {
    pub kind: String,
    pub message: String,
}

impl JoinError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self { kind: kind.into(), message: message.into() }
    }
}

impl From<crate::install::InstallError> for JoinError {
    fn from(e: crate::install::InstallError) -> Self {
        let kind = serde_json::to_value(&e).ok().and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(String::from));
        Self { kind: kind.unwrap_or_else(|| "install".into()), message: e.to_string() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_needs_what_the_player_confirmed() {
        let lobby: Lobby = serde_json::from_value(serde_json::json!({
            "id": "k3m9xq2wa7fd", "mashup": {"id": "sigf/example-mashup", "version": "1.0.0", "name": "Example Mashup"},
            "host": "host", "state": "open", "targets": [{"game": "minecraft", "address": "203.0.113.5:25565"}]
        }))
        .unwrap();
        let seen = |mashup: &str, version: &str, addr: &str| Confirmed {
            mashup: mashup.into(),
            version: version.into(),
            targets: [("minecraft".to_string(), addr.to_string())].into(),
        };
        assert!(seen("sigf/example-mashup", "1.0.0", "203.0.113.5:25565").matches(&lobby));
        assert!(!seen("sigf/example-mashup", "1.0.0", "198.51.100.9:25565").matches(&lobby), "address swapped");
        assert!(!seen("sigf/example-mashup", "1.0.1", "203.0.113.5:25565").matches(&lobby), "version changed");
        assert!(!seen("sigf/other", "1.0.0", "203.0.113.5:25565").matches(&lobby), "mashup changed");
        let none = Confirmed { mashup: "sigf/example-mashup".into(), version: "1.0.0".into(), targets: Default::default() };
        assert!(!none.matches(&lobby), "a target added since");
    }

    #[test]
    fn links_parse_to_the_lobby_id() {
        for ok in [
            "sigf://join/k3m9xq2wa7fd",
            "sigf://join/k3m9xq2wa7fd/",
            "SIGF://join/k3m9xq2wa7fd",
            "https://sigf.ai/join/k3m9xq2wa7fd",
            "https://sigf.ai/join/k3m9xq2wa7fd?utm=discord",
            "https://www.sigf.ai/join/k3m9xq2wa7fd#x",
            "  k3m9xq2wa7fd  ",
        ] {
            assert_eq!(parse_link(ok).as_deref(), Some("k3m9xq2wa7fd"), "{ok}");
        }
        for bad in [
            "",
            "sigf://join/",
            "sigf://join/k3m9xq2wa7f",
            "sigf://join/k3m9xq2wa7fdd",
            "sigf://join/K3M9XQ2WA7FD",
            "sigf://join/k3m9xq2wa7f1",
            "sigf://join/k3m9xq2wa7fl",
            "sigf://join/../../etc/x",
            "sigf://play/k3m9xq2wa7fd",
            "http://sigf.ai/join/k3m9xq2wa7fd",
            "https://evil.test/join/k3m9xq2wa7fd",
            "https://sigf.ai.evil.test/join/k3m9xq2wa7fd",
            "sigf://join/k3m9xq2wa7fd/extra",
            "sigf://join/é3m9xq2wa7fd",
        ] {
            assert_eq!(parse_link(bad), None, "{bad}");
        }
    }

    #[test]
    fn addresses_are_plain_host_port() {
        for ok in ["play.example.com:25565", "192.168.1.20:27015", "[2001:db8::1]:25565", "abc.joinmc.link:65535", "localhost:1"] {
            assert!(valid_address(ok), "{ok}");
        }
        for bad in [
            "",
            "example.com",
            "example.com:0",
            "example.com:65536",
            "example.com:080",
            "-exec:25565",
            "+quit:1",
            "a b:1",
            "example.com:25565 +exec x",
            "ex\"ample:1",
            "{app}:1",
            "http://x:1",
            "a..b:1",
            "-a.com:1",
            "a-.com:1",
            "2001:db8::1:25565",
            "[]:1",
            "[zz::1]:1",
        ] {
            assert!(!valid_address(bad), "{bad}");
        }
    }

    #[test]
    fn join_args_per_strategy() {
        assert_eq!(join_kind(Strategy::Mrpack, "minecraft"), JoinKind::Prism);
        assert_eq!(join_kind(Strategy::Profile, "tf2"), JoinKind::Connect);
        assert_eq!(join_kind(Strategy::Args, "quake"), JoinKind::Connect);
        assert_eq!(join_kind(Strategy::GameDirSnapshot, "gta5"), JoinKind::Mod);
        assert_eq!(join_kind(Strategy::Args, "doom"), JoinKind::Mod);

        assert_eq!(prism_args("sigf-gta5-blocky", Some("abc.joinmc.link:25565")), ["--launch", "sigf-gta5-blocky", "--server", "abc.joinmc.link:25565"]);
        assert_eq!(prism_args("sigf-gta5-blocky", None), ["--launch", "sigf-gta5-blocky"]);

        let base = vec!["-game".to_string(), "C:\\SIGF\\profiles\\x".to_string()];
        assert_eq!(store_join_args(JoinKind::Connect, &base, Some("10.0.0.5:27015")), ["-game", "C:\\SIGF\\profiles\\x", "+connect", "10.0.0.5:27015"]);
        assert_eq!(store_join_args(JoinKind::Connect, &base, None), base);
        assert_eq!(store_join_args(JoinKind::Mod, &base, Some("10.0.0.5:27015")), base);

        let f: serde_json::Value = serde_json::from_str(&join_file("k3m9xq2wa7fd", "10.0.0.5:7777", "Alex \"the\" host")).unwrap();
        assert_eq!(f["address"], "10.0.0.5:7777");
        assert_eq!(f["host"], "Alex \"the\" host");
    }

    #[test]
    fn minecraft_status_ping_bytes() {
        assert_eq!(varint(0), [0x00]);
        assert_eq!(varint(25565), [0xdd, 0xc7, 0x01]);
        assert_eq!(varint(-1), [0xff, 0xff, 0xff, 0xff, 0x0f]);
        for v in [0, 1, 127, 128, 25565, 2_097_151, i32::MAX, -1] {
            assert_eq!(read_varint(&mut &varint(v)[..]).unwrap(), v);
        }
        let req = status_request("localhost", 25565);
        // [len][0x00][-1 as varint][9]"localhost"[0x63 0xdd][0x01] then [1][0x00]
        let mut want = vec![19u8, 0x00, 0xff, 0xff, 0xff, 0xff, 0x0f, 9];
        want.extend_from_slice(b"localhost");
        want.extend_from_slice(&[0x63, 0xdd, 0x01, 0x01, 0x00]);
        assert_eq!(req, want);
        assert_eq!(parse_status(r#"{"version":{"name":"26.3"},"players":{"max":20,"online":3},"description":"x"}"#), Some((3, 20)));
        assert_eq!(parse_status(r#"{"players":{}}"#), None);
        assert_eq!(minecraft_players("-x:1"), None, "never dials a bad address");
    }

    #[test]
    fn lobby_json_reads_the_api_shape() {
        let l: Lobby = serde_json::from_str(r#"{ "id": "k3m9xq2wa7fd", "mashup": { "id": "sigf/gta5-blocky", "version": "1.0.0", "name": "Blocky" },
            "games": ["gta5", "minecraft"], "host": "Alex", "mode": "public", "players": 3, "maxPlayers": 20, "state": "open",
            "targets": [{ "game": "minecraft", "address": "abc.joinmc.link:25565", "join": "prism" }] }"#)
        .unwrap();
        assert_eq!(l.mashup.version, "1.0.0");
        assert_eq!(l.targets[0].address, "abc.joinmc.link:25565");
        let e: JoinError = crate::install::InstallError::NeedsLauncher { game: "minecraft".into(), launcher: "prism".into() }.into();
        assert_eq!(e.kind, "needsLauncher");
    }
}
