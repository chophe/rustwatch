use chrono::NaiveDate;
use rustwatch_core::{ActivityRecord, Store};

#[derive(Debug, Clone, Copy)]
pub enum ChartFormat {
    Terminal,
    Json,
    Html,
}

pub fn render_chart(store: &Store, date: NaiveDate, format: ChartFormat) -> anyhow::Result<String> {
    let activities = store.list_activities_for_date(date)?;
    match format {
        ChartFormat::Json => Ok(serde_json::to_string_pretty(&activities)?),
        ChartFormat::Html => Ok(render_html(&activities, date)),
        ChartFormat::Terminal => Ok(render_terminal(&activities, date)),
    }
}

fn render_terminal(activities: &[ActivityRecord], date: NaiveDate) -> String {
    let mut out = format!("Activity chart for {date}\n\n");
    if activities.is_empty() {
        out.push_str("No activities recorded.\n");
        return out;
    }

    for activity in activities {
        let duration = activity.ended_at - activity.started_at;
        let mins = duration.num_minutes().max(1);
        let blocks = (mins / 5).clamp(1, 20) as usize;
        let bar = "#".repeat(blocks);
        out.push_str(&format!(
            "{} {bar:<20} {} — {} ({mins}m)\n",
            activity.started_at.format("%H:%M"),
            activity.label,
            activity.apps.join(", "),
        ));
    }
    out
}

fn render_html(activities: &[ActivityRecord], date: NaiveDate) -> String {
    let mut rows = String::new();
    for activity in activities {
        let duration = activity.ended_at - activity.started_at;
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}m</td></tr>",
            activity.started_at.format("%H:%M"),
            activity.label,
            activity.category,
            duration.num_minutes().max(1),
        ));
    }

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>rustwatch {date}</title></head><body><h1>rustwatch activity chart</h1><table border=\"1\" cellpadding=\"6\"><tr><th>Time</th><th>Activity</th><th>Category</th><th>Duration</th></tr>{rows}</table></body></html>"
    )
}
