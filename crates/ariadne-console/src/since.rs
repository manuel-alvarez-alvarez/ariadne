//! The instant `--since` names, for the commands that print a session's
//! activity from then on.

use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};

/// An RFC 3339 instant, or an instant relative to command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Since(DateTime<Utc>);

impl Since {
    /// Whether an RFC 3339 `timestamp` is at or after this instant. A
    /// timestamp that does not parse is kept.
    pub fn includes(&self, timestamp: &str) -> bool {
        DateTime::parse_from_rfc3339(timestamp)
            .map(|at| at.with_timezone(&Utc) >= self.0)
            .unwrap_or(true)
    }
}

impl FromStr for Since {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Ok(at) = DateTime::parse_from_rfc3339(value) {
            return Ok(Self(at.with_timezone(&Utc)));
        }
        let Some((unit_at, unit)) = value.char_indices().next_back() else {
            return Err("use RFC 3339 or a duration such as 10m".to_string());
        };
        let number = &value[..unit_at];
        let count = number
            .parse::<u64>()
            .map_err(|_| "use RFC 3339 or a duration such as 10m".to_string())?;
        let seconds = match unit {
            's' => count.checked_mul(1),
            'm' => count.checked_mul(60),
            'h' => count.checked_mul(60 * 60),
            'd' => count.checked_mul(24 * 60 * 60),
            'w' => count.checked_mul(7 * 24 * 60 * 60),
            _ => None,
        }
        .ok_or_else(|| "use RFC 3339 or a duration such as 10m".to_string())?;
        let ago = chrono::Duration::from_std(Duration::from_secs(seconds))
            .map_err(|_| "duration is too large".to_string())?;
        Ok(Self(Utc::now() - ago))
    }
}

#[cfg(test)]
mod tests {
    use super::Since;

    #[test]
    fn a_multibyte_since_suffix_is_a_parse_error() {
        assert!("1é".parse::<Since>().is_err());
    }
}
