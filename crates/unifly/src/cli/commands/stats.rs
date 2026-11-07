//! Statistics command handlers.

use chrono::DateTime;
use unifly_api::Controller;

use crate::cli::args::{
    DpiGroupBy, GlobalOpts, OutputFormat, StatsArgs, StatsCommand, StatsInterval,
};
use crate::cli::error::CliError;
use crate::cli::output;

/// Convert a `StatsInterval` to the Session API string.
fn interval_str(interval: &StatsInterval) -> &'static str {
    match interval {
        StatsInterval::FiveMinutes => "5minutes",
        StatsInterval::Hourly => "hourly",
        StatsInterval::Daily => "daily",
        StatsInterval::Monthly => "monthly",
    }
}

/// Render `Vec<serde_json::Value>` in the chosen output format.
fn render_stats(data: &[serde_json::Value], format: &OutputFormat) -> String {
    match format {
        OutputFormat::JsonCompact => output::render_json_compact(data),
        OutputFormat::Yaml => output::render_yaml(data),
        // Dynamic fields -- fall back to pretty JSON for table/plain/json
        _ => output::render_json_pretty(data),
    }
}

fn parse_time(value: &str, field: &str) -> Result<i64, CliError> {
    if let Ok(ts) = value.parse::<i64>() {
        return ts.checked_mul(1000).ok_or_else(|| CliError::Validation {
            field: field.into(),
            reason: "Unix seconds are outside the supported millisecond range".into(),
        });
    }
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp_millis())
        .map_err(|_| CliError::Validation {
            field: field.into(),
            reason: format!("invalid timestamp '{value}' (use Unix seconds or RFC3339)"),
        })
}

fn parse_time_range(
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(Option<i64>, Option<i64>), CliError> {
    let start_ts = start.map(|s| parse_time(s, "start")).transpose()?;
    let end_ts = end.map(|s| parse_time(s, "end")).transpose()?;
    if let (Some(s), Some(e)) = (start_ts, end_ts)
        && s > e
    {
        return Err(CliError::Validation {
            field: "start".into(),
            reason: "start must be <= end".into(),
        });
    }
    Ok((start_ts, end_ts))
}

pub async fn handle(
    controller: &Controller,
    args: StatsArgs,
    global: &GlobalOpts,
) -> Result<(), CliError> {
    let data = match args.command {
        StatsCommand::Site(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_site_stats(interval, start, end, query.attrs.as_deref())
                .await?
        }
        StatsCommand::Device(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_device_stats_in_range(
                    interval,
                    start,
                    end,
                    query.macs.as_deref(),
                    query.attrs.as_deref(),
                )
                .await?
        }
        StatsCommand::Client(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_client_stats_in_range(
                    interval,
                    start,
                    end,
                    query.macs.as_deref(),
                    query.attrs.as_deref(),
                )
                .await?
        }
        StatsCommand::Gateway(query) => {
            let interval = interval_str(&query.interval);
            let (start, end) = parse_time_range(query.start.as_deref(), query.end.as_deref())?;
            controller
                .get_gateway_stats(interval, start, end, query.attrs.as_deref())
                .await?
        }
        StatsCommand::Dpi { group_by, macs } => {
            let gb = match group_by {
                DpiGroupBy::ByApp => "by_app",
                DpiGroupBy::ByCat => "by_cat",
            };
            controller.get_dpi_stats(gb, macs.as_deref()).await?
        }
    };

    let out = render_stats(&data, &global.output);
    output::print_output(&out, global.quiet);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_time, parse_time_range};

    #[test]
    fn stats_times_normalize_seconds_and_rfc3339_to_milliseconds() {
        assert_eq!(
            parse_time("1700000000", "start").expect("Unix seconds"),
            1_700_000_000_000
        );
        assert_eq!(
            parse_time("2023-11-14T22:13:20.123Z", "start").expect("RFC3339"),
            1_700_000_000_123
        );
        assert_eq!(
            parse_time("2023-11-15T00:13:20.123+02:00", "start").expect("offset timestamp"),
            1_700_000_000_123
        );
        assert_eq!(parse_time("-1", "start").expect("pre-epoch seconds"), -1000);
    }

    #[test]
    fn stats_times_reject_overflow_invalid_values_and_reversed_ranges() {
        for value in ["9223372036854775807", "-9223372036854775808", "not-a-time"] {
            assert!(parse_time(value, "end").is_err(), "{value}");
        }
        assert!(parse_time_range(Some("2"), Some("1")).is_err());
        assert_eq!(
            parse_time_range(None, Some("1")).expect("end-only range"),
            (None, Some(1000))
        );
        assert_eq!(
            parse_time_range(Some("1"), Some("1")).expect("equal bounds"),
            (Some(1000), Some(1000))
        );
    }
}
