//! The calculator: arithmetic as you type, and unit conversions like `5 km to mi`.

use std::f64::consts::{E, PI};

/// A result worth showing, with the text to copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub value: String,
    /// What was understood, like `5 km → mi`, shown under the value.
    pub detail: String,
}

/// Evaluates `query` if it reads as a calculation. Without `forced` (the `=` prefix), a bare
/// number or a word isn't one, so typing an app's name never shows a result.
pub fn answer(query: &str, forced: bool) -> Option<Answer> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if let Some(a) = convert(q) {
        return Some(a);
    }
    if !forced && !looks_like_math(q) {
        return None;
    }
    let v = eval(q).ok()?;
    if !v.is_finite() {
        return None;
    }
    Some(Answer { value: format(v), detail: q.to_string() })
}

fn looks_like_math(q: &str) -> bool {
    let digits = q.chars().any(|c| c.is_ascii_digit()) || q.contains("pi");
    let ops = q.chars().any(|c| matches!(c, '+' | '*' | '/' | '^' | '(' | '×' | '÷' | '%'))
        // A minus between things, not a leading one (`-5` alone isn't a sum).
        || q.trim_start_matches('-').contains('-')
        || FUNCTIONS.iter().any(|f| q.starts_with(f));
    digits && ops
}

/// Formats a number with up to 10 significant digits and no trailing zeros.
pub fn format(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    let magnitude = v.abs().log10().floor() as i32;
    if !(-6..15).contains(&magnitude) {
        let s = format!("{v:.9e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exp}");
    }
    let decimals = (9 - magnitude).clamp(0, 12) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

const FUNCTIONS: [&str; 13] =
    ["sqrt", "cbrt", "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "abs", "floor", "ceil"];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '_' => i += 1,
            // A thousands separator between digits.
            ',' if i > 0 && chars[i - 1].is_ascii_digit() && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                i += 1
            }
            '0' if matches!(chars.get(i + 1), Some('x' | 'b' | 'o')) => {
                let radix = match chars[i + 1] {
                    'x' => 16,
                    'b' => 2,
                    _ => 8,
                };
                let start = i + 2;
                let mut j = start;
                while j < chars.len() && chars[j].is_digit(radix) {
                    j += 1;
                }
                let digits: String = chars[start..j].iter().collect();
                let v = i64::from_str_radix(&digits, radix).map_err(|e| e.to_string())?;
                out.push(Tok::Num(v as f64));
                i = j;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',') {
                    // Commas only as thousands separators.
                    if chars[i] == ',' && !chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                        break;
                    }
                    i += 1;
                }
                // An exponent: 1e3, 2.5E-4.
                if i < chars.len()
                    && (chars[i] == 'e' || chars[i] == 'E')
                    && (chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
                        || (matches!(chars.get(i + 1), Some('-' | '+'))
                            && chars.get(i + 2).is_some_and(|c| c.is_ascii_digit())))
                {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let text: String = chars[start..i].iter().filter(|c| **c != ',').collect();
                out.push(Tok::Num(text.parse().map_err(|_| format!("bad number {text:?}"))?));
            }
            c if c.is_alphabetic() || c == 'π' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == 'π') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect::<String>().to_lowercase()));
            }
            '+' | '-' | '*' | '/' | '^' | '%' | '!' => {
                // `**` is a power, as in Python.
                if c == '*' && chars.get(i + 1) == Some(&'*') {
                    out.push(Tok::Op('^'));
                    i += 2;
                } else {
                    out.push(Tok::Op(c));
                    i += 1;
                }
            }
            '×' | '·' => {
                out.push(Tok::Op('*'));
                i += 1;
            }
            '÷' => {
                out.push(Tok::Op('/'));
                i += 1;
            }
            '−' => {
                out.push(Tok::Op('-'));
                i += 1;
            }
            '(' | '[' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' | ']' => {
                out.push(Tok::RParen);
                i += 1;
            }
            other => return Err(format!("unexpected {other:?}")),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// Whether the last operand parsed ended in `%`.
    percent_last: bool,
}

/// Evaluates an arithmetic expression.
///
/// `+ - * / ^`, `%` as a percentage (`50%` is 0.5, `200 + 10%` is 220), `of` (`12% of 340`), `mod`, `!`,
/// parentheses, implicit multiplication (`2pi`, `3(4+1)`), `pi`, `e`, and the functions in
/// [`FUNCTIONS`] (trig in radians; `log` is base 10).
pub fn eval(s: &str) -> Result<f64, String> {
    let mut p = Parser { toks: lex(s)?, pos: 0, percent_last: false };
    if p.toks.is_empty() {
        return Err("empty".into());
    }
    let v = p.sum()?;
    if let Some(t) = p.toks.get(p.pos) {
        return Err(format!("unexpected {t:?}"));
    }
    Ok(v)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.product()?;
        loop {
            match self.peek() {
                Some(Tok::Op(op @ ('+' | '-'))) => {
                    let op = *op;
                    self.pos += 1;
                    let rhs = self.product()?;
                    // `x + 10%` adds ten percent of x, as calculators do.
                    let rhs = if self.percent_last { v * rhs } else { rhs };
                    v = if op == '+' { v + rhs } else { v - rhs };
                }
                _ => return Ok(v),
            }
        }
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op(op @ ('*' | '/'))) => {
                    let op = *op;
                    self.pos += 1;
                    let rhs = self.unary()?;
                    v = if op == '*' { v * rhs } else { v / rhs };
                }
                // 12% of 340.
                Some(Tok::Ident(w)) if w == "of" => {
                    self.pos += 1;
                    v *= self.unary()?;
                }
                Some(Tok::Ident(w)) if w == "mod" => {
                    self.pos += 1;
                    let rhs = self.unary()?;
                    v = v.rem_euclid(rhs);
                }
                // Implicit multiplication: 2pi, 3(1+2), 2 sqrt 4.
                Some(Tok::Num(_) | Tok::LParen | Tok::Ident(_)) => {
                    let rhs = self.unary()?;
                    v *= rhs;
                }
                _ => return Ok(v),
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.postfix()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.pos += 1;
            // Right associative, and binds tighter than a leading minus: -2^2 is -4.
            let exp = self.unary()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<f64, String> {
        let mut v = self.atom()?;
        self.percent_last = false;
        loop {
            match self.peek() {
                Some(Tok::Op('%')) => {
                    self.pos += 1;
                    v /= 100.0;
                    self.percent_last = true;
                }
                Some(Tok::Op('!')) => {
                    self.pos += 1;
                    if v < 0.0 || v.fract() != 0.0 || v > 170.0 {
                        return Err("factorial needs a whole number up to 170".into());
                    }
                    v = (1..=v as u32).map(f64::from).product();
                }
                _ => return Ok(v),
            }
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(n),
            Some(Tok::LParen) => {
                let v = self.sum()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    // Forgive a missing `)` at the end, which is common mid-typing.
                    None => Ok(v),
                    Some(t) => Err(format!("expected ), found {t:?}")),
                }
            }
            Some(Tok::Ident(name)) => match name.as_str() {
                "pi" | "π" => Ok(PI),
                "e" => Ok(E),
                "tau" => Ok(2.0 * PI),
                f if FUNCTIONS.contains(&f) => {
                    let x = self.power()?;
                    Ok(match f {
                        "sqrt" => x.sqrt(),
                        "cbrt" => x.cbrt(),
                        "sin" => x.sin(),
                        "cos" => x.cos(),
                        "tan" => x.tan(),
                        "asin" => x.asin(),
                        "acos" => x.acos(),
                        "atan" => x.atan(),
                        "ln" => x.ln(),
                        "log" => x.log10(),
                        "abs" => x.abs(),
                        "floor" => x.floor(),
                        _ => x.ceil(),
                    })
                }
                other => Err(format!("unknown name {other:?}")),
            },
            Some(t) => Err(format!("unexpected {t:?}")),
            None => Err("expression ends early".into()),
        }
    }
}

/// A unit: its kind, and how to get to the kind's base unit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Length,
    Mass,
    Time,
    Data,
    Volume,
    Speed,
    Area,
}

/// (names, kind, factor to the base unit). Temperatures are handled on their own.
const UNITS: &[(&[&str], Kind, f64)] = &[
    (&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Kind::Length, 0.001),
    (&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Kind::Length, 0.01),
    (&["m", "meter", "meters", "metre", "metres"], Kind::Length, 1.0),
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Kind::Length, 1000.0),
    (&["in", "inch", "inches", "\""], Kind::Length, 0.0254),
    (&["ft", "foot", "feet", "'"], Kind::Length, 0.3048),
    (&["yd", "yard", "yards"], Kind::Length, 0.9144),
    (&["mi", "mile", "miles"], Kind::Length, 1609.344),
    (&["nmi", "nauticalmile"], Kind::Length, 1852.0),
    (&["mg", "milligram", "milligrams"], Kind::Mass, 0.001),
    (&["g", "gram", "grams"], Kind::Mass, 1.0),
    (&["kg", "kilogram", "kilograms", "kilo", "kilos"], Kind::Mass, 1000.0),
    (&["t", "tonne", "tonnes"], Kind::Mass, 1_000_000.0),
    (&["oz", "ounce", "ounces"], Kind::Mass, 28.349_523_125),
    (&["lb", "lbs", "pound", "pounds"], Kind::Mass, 453.592_37),
    (&["st", "stone", "stones"], Kind::Mass, 6_350.293_18),
    (&["ms", "millisecond", "milliseconds"], Kind::Time, 0.001),
    (&["s", "sec", "secs", "second", "seconds"], Kind::Time, 1.0),
    (&["min", "mins", "minute", "minutes"], Kind::Time, 60.0),
    (&["h", "hr", "hrs", "hour", "hours"], Kind::Time, 3600.0),
    (&["d", "day", "days"], Kind::Time, 86_400.0),
    (&["wk", "week", "weeks"], Kind::Time, 604_800.0),
    (&["yr", "year", "years"], Kind::Time, 31_557_600.0),
    (&["b", "byte", "bytes"], Kind::Data, 1.0),
    (&["bit", "bits"], Kind::Data, 0.125),
    (&["kb", "kilobyte", "kilobytes"], Kind::Data, 1e3),
    (&["mb", "megabyte", "megabytes"], Kind::Data, 1e6),
    (&["gb", "gigabyte", "gigabytes"], Kind::Data, 1e9),
    (&["tb", "terabyte", "terabytes"], Kind::Data, 1e12),
    (&["kib", "kibibyte", "kibibytes"], Kind::Data, 1024.0),
    (&["mib", "mebibyte", "mebibytes"], Kind::Data, 1_048_576.0),
    (&["gib", "gibibyte", "gibibytes"], Kind::Data, 1_073_741_824.0),
    (&["tib", "tebibyte", "tebibytes"], Kind::Data, 1_099_511_627_776.0),
    (&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], Kind::Volume, 0.001),
    (&["l", "liter", "liters", "litre", "litres"], Kind::Volume, 1.0),
    (&["gal", "gallon", "gallons"], Kind::Volume, 3.785_411_784),
    (&["qt", "quart", "quarts"], Kind::Volume, 0.946_352_946),
    (&["pt", "pint", "pints"], Kind::Volume, 0.473_176_473),
    (&["cup", "cups"], Kind::Volume, 0.236_588_236_5),
    (&["floz"], Kind::Volume, 0.029_573_529_562_5),
    (&["kmh", "kph", "km/h"], Kind::Speed, 1.0 / 3.6),
    (&["mph", "mi/h"], Kind::Speed, 0.447_04),
    (&["m/s", "mps"], Kind::Speed, 1.0),
    (&["kn", "knot", "knots"], Kind::Speed, 0.514_444),
    (&["m2", "sqm", "m²"], Kind::Area, 1.0),
    (&["km2", "km²"], Kind::Area, 1e6),
    (&["ft2", "sqft", "ft²"], Kind::Area, 0.092_903_04),
    (&["acre", "acres"], Kind::Area, 4_046.856_422_4),
    (&["ha", "hectare", "hectares"], Kind::Area, 10_000.0),
];

const TEMPERATURES: &[(&[&str], &str)] =
    &[(&["c", "°c", "celsius"], "°C"), (&["f", "°f", "fahrenheit"], "°F"), (&["k", "kelvin"], "K")];

fn unit(name: &str) -> Option<(Kind, f64, &'static str)> {
    let n = name.to_lowercase();
    UNITS.iter().find(|(names, _, _)| names.contains(&n.as_str())).map(|(names, k, f)| (*k, *f, names[0]))
}

fn temperature(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    TEMPERATURES.iter().find(|(names, _)| names.contains(&n.as_str())).map(|(_, s)| *s)
}

/// `5 km to mi`, `100f in c`, `2.5 GiB as MB`, `0xff to bin`.
fn convert(q: &str) -> Option<Answer> {
    let lower = q.to_lowercase();
    let (lhs, to) = [" to ", " in ", " as ", " → ", " -> "].iter().find_map(|sep| lower.rsplit_once(sep))?;
    let (lhs, to) = (lhs.trim(), to.trim());

    if let Some(radix) = match to {
        "hex" | "hexadecimal" => Some(16),
        "bin" | "binary" => Some(2),
        "oct" | "octal" => Some(8),
        "dec" | "decimal" => Some(10),
        _ => None,
    } {
        let v = eval(lhs).ok()?;
        if v.fract() != 0.0 || v.abs() > 9.0e15 {
            return None;
        }
        let n = v as i64;
        let value = match radix {
            16 => format!("0x{n:x}"),
            2 => format!("0b{n:b}"),
            8 => format!("0o{n:o}"),
            _ => n.to_string(),
        };
        return Some(Answer { value, detail: format!("{lhs} → {to}") });
    }

    // Split the number from its unit: the unit is the trailing letters.
    let split = lhs.rfind(|c: char| c.is_ascii_digit() || c == ')' || c == '.' || c == ' ')? + 1;
    let (amount, from) = (lhs[..split].trim(), lhs[split..].trim());
    let amount = if amount.is_empty() { return None } else { eval(amount).ok()? };

    if let (Some(f), Some(t)) = (temperature(from), temperature(to)) {
        let celsius = match f {
            "°C" => amount,
            "°F" => (amount - 32.0) * 5.0 / 9.0,
            _ => amount - 273.15,
        };
        let out = match t {
            "°C" => celsius,
            "°F" => celsius * 9.0 / 5.0 + 32.0,
            _ => celsius + 273.15,
        };
        let out = (out * 100.0).round() / 100.0;
        return Some(Answer {
            value: format!("{} {t}", format(out)), detail: format!("{} {f} → {t}", format(amount))
        });
    }

    let (fk, ff, fname) = unit(from)?;
    let (tk, tf, tname) = unit(to)?;
    if fk != tk {
        return None;
    }
    let out = amount * ff / tf;
    Some(Answer {
        value: format!("{} {tname}", format(out)), detail: format!("{} {fname} → {tname}", format(amount))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> String {
        format(eval(s).unwrap_or_else(|e| panic!("{s}: {e}")))
    }

    #[test]
    fn arithmetic() {
        assert_eq!(v("1 + 2 * 3"), "7");
        assert_eq!(v("(1 + 2) * 3"), "9");
        assert_eq!(v("2^10"), "1024");
        assert_eq!(v("2**3**2"), "512");
        assert_eq!(v("-2^2"), "-4");
        assert_eq!(v("10 / 4"), "2.5");
        assert_eq!(v("1/3"), "0.3333333333");
        assert_eq!(v("2pi"), "6.283185307");
        assert_eq!(v("3(4+1)"), "15");
        assert_eq!(v("sqrt 16 + 1"), "5");
        assert_eq!(v("sqrt(2)^2"), "2");
        assert_eq!(v("5!"), "120");
        assert_eq!(v("17 mod 5"), "2");
        assert_eq!(v("1,000 * 3"), "3000");
        assert_eq!(v("0xff + 1"), "256");
        assert_eq!(v("1e3 + 2.5E-1"), "1000.25");
        assert_eq!(v("6 × 7 ÷ 2"), "21");
        assert_eq!(v("(2 + 3"), "5");
    }

    #[test]
    fn percentages() {
        assert_eq!(v("50%"), "0.5");
        assert_eq!(v("200 + 10%"), "220");
        assert_eq!(v("12% of 340 + 2^8"), "296.8");
        assert_eq!(v("200 - 25%"), "150");
        assert_eq!(v("20% * 50"), "10");
    }

    #[test]
    fn only_math_shows_unasked() {
        assert!(answer("firefox", false).is_none());
        assert!(answer("42", false).is_none());
        assert!(answer("-5", false).is_none());
        assert!(answer("2048-game", false).is_none());
        assert_eq!(answer("42", true).unwrap().value, "42");
        assert_eq!(answer("12*12", false).unwrap().value, "144");
        assert_eq!(answer("10-3", false).unwrap().value, "7");
    }

    #[test]
    fn conversions() {
        assert_eq!(answer("5 km to mi", false).unwrap().value, "3.106855961 mi");
        assert_eq!(answer("100f to c", false).unwrap().value, "37.78 °C");
        assert_eq!(answer("0 c in k", false).unwrap().value, "273.15 K");
        assert_eq!(answer("1 GiB to MB", false).unwrap().value, "1073.741824 mb");
        assert_eq!(answer("2 h in min", false).unwrap().value, "120 min");
        assert_eq!(answer("255 to hex", false).unwrap().value, "0xff");
        assert_eq!(answer("0b1010 to dec", false).unwrap().value, "10");
        assert!(answer("5 km to kg", false).is_none());
        assert!(answer("go to bed", false).is_none());
    }

    #[test]
    fn formatting() {
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(1e20), "1e20");
        assert_eq!(format(1.5e-9), "1.5e-9");
        assert_eq!(format(-0.5), "-0.5");
    }
}
