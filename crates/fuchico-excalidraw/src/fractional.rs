//! Port of the `fractional-indexing` key generator Excalidraw uses for the
//! `index` field (base-62 digits, variable-length integer head).

const DIGITS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

fn digit_index(c: u8) -> Option<usize> {
    DIGITS.iter().position(|&d| d == c)
}

fn integer_length(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some((head - b'a') as usize + 2),
        b'A'..=b'Z' => Some((b'Z' - head) as usize + 2),
        _ => None,
    }
}

fn integer_part(key: &str) -> Option<&str> {
    let len = integer_length(*key.as_bytes().first()?)?;
    key.get(..len)
}

fn validate(key: &str) -> bool {
    if key == SMALLEST_INTEGER {
        return false;
    }
    let Some(i) = integer_part(key) else { return false };
    let f = &key[i.len()..];
    !f.ends_with('0') && key.bytes().all(|b| digit_index(b).is_some())
}

fn midpoint(a: &str, b: Option<&str>) -> String {
    // a < b lexicographically; neither ends in '0'.
    if let Some(b) = b {
        let mut n = 0;
        let (ab, bb) = (a.as_bytes(), b.as_bytes());
        while n < bb.len() && ab.get(n).copied().unwrap_or(b'0') == bb[n] {
            n += 1;
        }
        if n > 0 {
            return format!(
                "{}{}",
                &b[..n],
                midpoint(a.get(n..).unwrap_or(""), Some(&b[n..]))
            );
        }
    }
    let da = a.bytes().next().and_then(digit_index).unwrap_or(0);
    let db = b
        .and_then(|b| b.bytes().next())
        .and_then(digit_index)
        .unwrap_or(DIGITS.len());
    if db - da > 1 {
        let mid = (da + db) / 2;
        (DIGITS[mid] as char).to_string()
    } else if b.map(|b| b.len() > 1).unwrap_or(false) {
        b.unwrap()[..1].to_string()
    } else {
        format!(
            "{}{}",
            DIGITS[da] as char,
            midpoint(a.get(1..).unwrap_or(""), None)
        )
    }
}

fn increment_integer(x: &str) -> Option<String> {
    let head = x.as_bytes()[0];
    let mut digs: Vec<u8> = x.as_bytes()[1..].to_vec();
    let mut carry = true;
    let mut i = digs.len();
    while carry && i > 0 {
        i -= 1;
        let d = digit_index(digs[i])? + 1;
        if d == DIGITS.len() {
            digs[i] = b'0';
        } else {
            digs[i] = DIGITS[d];
            carry = false;
        }
    }
    if carry {
        if head == b'Z' {
            return Some("a0".into());
        }
        if head == b'z' {
            return None;
        }
        let h = head + 1;
        if h > b'a' {
            digs.push(b'0');
        } else {
            digs.pop();
        }
        let mut s = vec![h];
        s.extend(digs);
        return String::from_utf8(s).ok();
    }
    let mut s = vec![head];
    s.extend(digs);
    String::from_utf8(s).ok()
}

fn decrement_integer(x: &str) -> Option<String> {
    let head = x.as_bytes()[0];
    let mut digs: Vec<u8> = x.as_bytes()[1..].to_vec();
    let mut borrow = true;
    let mut i = digs.len();
    while borrow && i > 0 {
        i -= 1;
        let d = digit_index(digs[i])? as i32 - 1;
        if d == -1 {
            digs[i] = *DIGITS.last().unwrap();
        } else {
            digs[i] = DIGITS[d as usize];
            borrow = false;
        }
    }
    if borrow {
        if head == b'a' {
            return Some(format!("Z{}", *DIGITS.last().unwrap() as char));
        }
        if head == b'A' {
            return None;
        }
        let h = head - 1;
        if h < b'Z' {
            digs.push(*DIGITS.last().unwrap());
        } else {
            digs.pop();
        }
        let mut s = vec![h];
        s.extend(digs);
        return String::from_utf8(s).ok();
    }
    let mut s = vec![head];
    s.extend(digs);
    String::from_utf8(s).ok()
}

/// A key strictly between `a` and `b` (either may be open). Invalid inputs
/// are treated as open ends so this never fails on odd files.
pub fn key_between(a: Option<&str>, b: Option<&str>) -> String {
    let a = a.filter(|k| validate(k));
    let b = b.filter(|k| validate(k));
    let (a, b) = match (a, b) {
        (Some(x), Some(y)) if x >= y => (Some(x), None),
        other => other,
    };
    match (a, b) {
        (None, None) => "a0".into(),
        (None, Some(b)) => {
            let ib = integer_part(b).unwrap();
            let fb = &b[ib.len()..];
            if ib == SMALLEST_INTEGER {
                return format!("{}{}", ib, midpoint("", Some(fb)));
            }
            if ib.len() < b.len() {
                return ib.to_string();
            }
            decrement_integer(ib).unwrap_or_else(|| "a0".into())
        }
        (Some(a), None) => {
            let ia = integer_part(a).unwrap();
            let fa = &a[ia.len()..];
            match increment_integer(ia) {
                Some(i) => i,
                None => format!("{}{}", ia, midpoint(fa, None)),
            }
        }
        (Some(a), Some(b)) => {
            let ia = integer_part(a).unwrap();
            let fa = &a[ia.len()..];
            let ib = integer_part(b).unwrap();
            let fb = &b[ib.len()..];
            if ia == ib {
                return format!("{}{}", ia, midpoint(fa, Some(fb)));
            }
            match increment_integer(ia) {
                Some(i) if i.as_str() < b => i,
                _ => format!("{}{}", ia, midpoint(fa, None)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(key_between(None, None), "a0");
        assert_eq!(key_between(Some("a0"), None), "a1");
        assert_eq!(key_between(Some("a1"), None), "a2");
        assert_eq!(key_between(None, Some("a0")), "Zz");
        assert_eq!(key_between(Some("a0"), Some("a1")), "a0V");
        assert_eq!(key_between(Some("az"), None), "b00");
        let k = key_between(Some("a0V"), Some("a1"));
        assert!(k.as_str() > "a0V" && k.as_str() < "a1");
    }
}
