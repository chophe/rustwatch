-- D-10 annotation notes (hotkey shoot-first-prompt-second) plus D-18
-- idle marking on segments. Reports exclude idle segments (Phase 5).
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY NOT NULL,
    ts TEXT NOT NULL,
    note TEXT NOT NULL,
    screenshot_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_notes_screenshot_id ON notes(screenshot_id);

ALTER TABLE segments ADD COLUMN idle INTEGER NOT NULL DEFAULT 0;
