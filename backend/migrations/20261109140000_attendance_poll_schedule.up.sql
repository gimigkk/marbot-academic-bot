ALTER TABLE attendance_subscriptions
    ADD COLUMN next_check_at TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE attendance_subscriptions
SET next_check_at = now() + (student_id % 900) * interval '1 second';
CREATE INDEX attendance_subscriptions_next_check ON attendance_subscriptions(next_check_at);
