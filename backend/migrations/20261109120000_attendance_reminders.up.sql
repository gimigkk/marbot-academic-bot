CREATE TABLE attendance_subscriptions (
    user_id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL,
    nim TEXT NOT NULL,
    student_id BIGINT NOT NULL CHECK (student_id > 0),
    generation UUID NOT NULL,
    baseline JSONB NOT NULL,
    last_checked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Baseline updates and pending reminders are committed together. A delivery is
-- claimed durably BEFORE calling WAHA: ambiguous sends are never retried.
CREATE TABLE attendance_reminders (
    id UUID PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES attendance_subscriptions(user_id) ON DELETE CASCADE,
    generation UUID NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    attempted_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ
);
CREATE INDEX attendance_reminders_pending ON attendance_reminders(created_at)
    WHERE attempted_at IS NULL;
