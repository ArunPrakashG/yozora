//! Currency conversion with the European Central Bank's reference rates: `100 usd to inr`,
//! `$20 in eur`, `5k yen to usd`. The rates are fetched at most twice a day and cached, and
//! conversion never waits for the network.

use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use crate::calc::{self, Answer};

const URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
/// The ECB publishes once each working day, around 16:00 CET.
const STALE: Duration = Duration::from_secs(12 * 3600);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rates {
    /// Units per euro.
    pub per_eur: HashMap<String, f64>,
    /// The day the rates are for, as the ECB writes it.
    pub date: String,
}

impl Rates {
    pub fn parse(xml: &str) -> Option<Self> {
        let mut per_eur = HashMap::from([("EUR".to_string(), 1.0)]);
        let attr = |chunk: &str, name: &str| -> Option<String> {
            let start = chunk.find(&format!("{name}='"))? + name.len() + 2;
            let end = chunk[start..].find('\'')? + start;
            Some(chunk[start..end].to_string())
        };
        let mut date = String::new();
        for chunk in xml.split("<Cube ").skip(1) {
            if let Some(t) = attr(chunk, "time") {
                date = t;
            }
            if let (Some(c), Some(r)) = (attr(chunk, "currency"), attr(chunk, "rate"))
                && let Ok(r) = r.parse::<f64>()
                && r > 0.0
            {
                per_eur.insert(c, r);
            }
        }
        (per_eur.len() > 1).then_some(Self { per_eur, date })
    }

    fn convert(&self, amount: f64, from: &str, to: &str) -> Option<f64> {
        Some(amount / self.per_eur.get(from)? * self.per_eur.get(to)?)
    }
}

fn cache_file() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| "/tmp".into())
        .join("yozora/launch/ecb-rates.xml")
}

/// The cached rates, if any.
pub fn cached() -> Option<Rates> {
    Rates::parse(&std::fs::read_to_string(cache_file()).ok()?)
}

/// Fetches the rates if the cache is old or missing, and returns them if they changed.
/// Blocks; call it off the UI thread.
pub fn refresh() -> Option<Rates> {
    let path = cache_file();
    let age =
        std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
    if age.is_some_and(|a| a < STALE) {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let part = path.with_extension("part");
    let ok = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "10", "-o"])
        .arg(&part)
        .arg(URL)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    let rates = ok.then(|| std::fs::read_to_string(&part).ok().and_then(|t| Rates::parse(&t))).flatten();
    match rates {
        Some(r) => {
            std::fs::rename(&part, &path).ok()?;
            Some(r)
        }
        None => {
            let _ = std::fs::remove_file(&part);
            // Try again in an hour, not on every open: touch the old file if there is one.
            if path.exists() {
                let _ = std::fs::File::options()
                    .append(true)
                    .open(&path)
                    .and_then(|f| f.set_modified(SystemTime::now() - STALE + Duration::from_secs(3600)));
            }
            tracing::debug!("couldn't fetch currency rates");
            None
        }
    }
}

/// Symbols and names people type for the common currencies.
const NAMES: &[(&str, &str)] = &[
    ("$", "USD"),
    ("dollar", "USD"),
    ("dollars", "USD"),
    ("€", "EUR"),
    ("euro", "EUR"),
    ("euros", "EUR"),
    ("£", "GBP"),
    ("pound", "GBP"),
    ("pounds", "GBP"),
    ("¥", "JPY"),
    ("yen", "JPY"),
    ("₹", "INR"),
    ("rupee", "INR"),
    ("rupees", "INR"),
    ("rs", "INR"),
    ("₩", "KRW"),
    ("won", "KRW"),
    ("yuan", "CNY"),
    ("rmb", "CNY"),
    ("franc", "CHF"),
    ("francs", "CHF"),
    ("₺", "TRY"),
    ("lira", "TRY"),
    ("zł", "PLN"),
    ("zloty", "PLN"),
    ("real", "BRL"),
    ("reais", "BRL"),
    ("baht", "THB"),
];

fn currency(s: &str, rates: &Rates) -> Option<String> {
    let s = s.trim().to_lowercase();
    if let Some((_, code)) = NAMES.iter().find(|(n, _)| *n == s) {
        return Some(code.to_string());
    }
    let code = s.to_uppercase();
    (code.len() == 3 && rates.per_eur.contains_key(&code)).then_some(code)
}

/// Splits `$100`, `100$`, `100 usd` or `5k yen` into the amount and the currency.
fn amount_and_currency(s: &str, rates: &Rates) -> Option<(f64, String)> {
    let s = s.trim();
    // A symbol in front: `$100`, `€ 5`.
    if let Some(sym) =
        NAMES.iter().map(|(n, _)| *n).filter(|n| !n.chars().all(char::is_alphabetic)).find(|n| s.starts_with(n))
    {
        return Some((amount(&s[sym.len()..])?, currency(sym, rates)?));
    }
    // A currency after: the last word, or a symbol stuck to the number.
    let (num, cur) = match s.rsplit_once(char::is_whitespace) {
        Some((n, c)) => (n, c),
        None => {
            let i = s.char_indices().find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | ',' | 'k' | 'm')))?.0;
            (&s[..i], &s[i..])
        }
    };
    Some((amount(num)?, currency(cur, rates)?))
}

/// `100`, `1,500`, `5k`, `2.5m`, or a sum like `20*3`.
fn amount(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', "");
    let (s, mult) = match s.strip_suffix(['k', 'K']) {
        Some(n) => (n.to_string(), 1e3),
        None => match s.strip_suffix(['m', 'M']) {
            Some(n) => (n.to_string(), 1e6),
            None => (s, 1.0),
        },
    };
    let v = s.trim().parse::<f64>().ok().or_else(|| calc::eval(&s).ok())?;
    (v.is_finite() && v >= 0.0).then_some(v * mult)
}

/// Two decimals with thousands separators: 8,345.12.
fn money(v: f64) -> String {
    let fixed = format!("{v:.2}");
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped}.{frac}")
}

pub fn answer(q: &str, rates: &Rates) -> Option<Answer> {
    let lower = q.trim().to_lowercase();
    let (left, to) = lower.split_once(" to ").or_else(|| lower.split_once(" in "))?;
    let to = currency(to, rates)?;
    let (amount, from) = amount_and_currency(left, rates)?;
    if from == to {
        return None;
    }
    let v = rates.convert(amount, &from, &to)?;
    Some(Answer {
        value: format!("{} {to}", money(v)),
        detail: format!("{} {from} → {to} · ECB rates of {}", calc::format(amount), rates.date),
    })
}

/// What Enter copies: the number alone, without separators.
pub fn plain(value: &str) -> String {
    value.split_whitespace().next().unwrap_or(value).replace(',', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<gesmes:Envelope><Cube><Cube time='2026-10-13'><Cube currency='USD' rate='1.1'/><Cube currency='INR' rate='92.4'/><Cube currency='JPY' rate='165.0'/><Cube currency='GBP' rate='0.88'/></Cube></Cube></gesmes:Envelope>";

    #[test]
    fn converts() {
        let r = Rates::parse(XML).unwrap();
        assert_eq!(r.date, "2026-10-13");
        let a = answer("100 usd to inr", &r).unwrap();
        assert_eq!(a.value, "8,400.00 INR");
        assert!(a.detail.contains("100 USD → INR"));
        assert_eq!(answer("$100 in eur", &r).unwrap().value, "90.91 EUR");
        assert_eq!(answer("100€ to usd", &r).unwrap().value, "110.00 USD");
        assert_eq!(answer("5k yen to usd", &r).unwrap().value, "33.33 USD");
        assert_eq!(answer("1,000 pounds to eur", &r).unwrap().value, "1,136.36 EUR");
        assert_eq!(answer("20*3 usd to eur", &r).unwrap().value, "54.55 EUR");
        assert_eq!(plain("1,136.36 EUR"), "1136.36");
        // Not money.
        assert!(answer("5 km to mi", &r).is_none());
        assert!(answer("usd to inr", &r).is_none());
        assert!(answer("100 xyz to inr", &r).is_none());
    }
}
