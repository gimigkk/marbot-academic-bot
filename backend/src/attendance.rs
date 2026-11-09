//! Opt-in attendance-count reminders. No user JWTs or attendance writes.
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{types::Json, PgPool};
use std::{collections::BTreeMap, future::Future, time::Duration};
use uuid::Uuid;

#[derive(Debug)]
pub enum ReminderAction {
    Enable(String),
    Disable,
}

pub const USAGE: &str = "⚙️ *SETTING PENGINGAT ABSENSI*\n\n#remind-absen <NIM> — aktifkan untuk NIM kamu sendiri\n#remind-absen off — berhenti dan hapus data pengingat\n\n_Kirim lewat chat pribadi Maarbot ya!_";

pub fn parse_action(args: &[&str]) -> Option<ReminderAction> {
    match args {
        [arg] if arg.eq_ignore_ascii_case("off") => Some(ReminderAction::Disable),
        [nim]
            if (5..=20).contains(&nim.len())
                && nim.as_bytes()[0].is_ascii_alphabetic()
                && nim.bytes().all(|c| c.is_ascii_alphanumeric())
                && nim.bytes().any(|c| c.is_ascii_digit()) =>
        {
            Some(ReminderAction::Enable(nim.to_ascii_uppercase()))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttendanceCount {
    pub course: String,
    pub class: String,
    pub missed: u32,
    pub meetings: u32,
}

pub type Snapshot = BTreeMap<String, AttendanceCount>;

#[derive(Debug)]
pub struct Change {
    pub course: String,
    pub class: String,
    pub previous: u32,
    pub current: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Course {
    kode_mk: String,
    nama_mk: String,
    detail_rekap: Vec<Class>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Class {
    jenis_kelas: String,
    kelas_paralel: String,
    jumlah_hadir: u32,
    jumlah_tidak_hadir: u32,
    jumlah_pertemuan: u32,
}

pub fn parse_snapshot(value: Value) -> Result<Snapshot> {
    let courses: Vec<Course> = serde_json::from_value(value).context("Invalid recap schema")?;
    let mut result = Snapshot::new();
    for course in courses {
        ensure!(
            !course.kode_mk.is_empty() && !course.nama_mk.is_empty(),
            "Missing course identity"
        );
        for class in course.detail_rekap {
            ensure!(
                !class.jenis_kelas.is_empty() && !class.kelas_paralel.is_empty(),
                "Missing class identity"
            );
            ensure!(
                u64::from(class.jumlah_hadir) + u64::from(class.jumlah_tidak_hadir)
                    == u64::from(class.jumlah_pertemuan),
                "Inconsistent attendance totals"
            );
            // JSON tuple encoding avoids ambiguous concatenated identifiers.
            let key = serde_json::to_string(&(
                &course.kode_mk,
                &class.jenis_kelas,
                &class.kelas_paralel,
            ))?;
            let count = AttendanceCount {
                course: course.nama_mk.clone(),
                class: format!("{}{}", class.jenis_kelas, class.kelas_paralel),
                missed: class.jumlah_tidak_hadir,
                meetings: class.jumlah_pertemuan,
            };
            ensure!(
                result.insert(key, count).is_none(),
                "Duplicate course/class identity"
            );
        }
    }
    Ok(result)
}

pub fn compare(previous: &Snapshot, current: &Snapshot) -> Vec<Change> {
    current
        .iter()
        .filter_map(|(key, now)| {
            let old = previous.get(key)?; // A new class establishes a silent baseline.
            (now.missed > old.missed && now.meetings >= old.meetings).then(|| Change {
                course: now.course.clone(),
                class: now.class.clone(),
                previous: old.missed,
                current: now.missed,
            })
        })
        .collect()
}

pub fn resolve_student(value: Value, nim: &str) -> Result<i64> {
    let items = value
        .get("Items")
        .and_then(Value::as_array)
        .context("Missing student result list")?;
    ensure!(items.len() == 1, "NIM must resolve to exactly one student");
    let item = &items[0];
    ensure!(
        item.get("NIM")
            .and_then(Value::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case(nim)),
        "NIM mismatch"
    );
    let id = item
        .get("MahasiswaID")
        .and_then(Value::as_i64)
        .context("Missing student ID")?;
    ensure!(id > 0, "Invalid student ID");
    Ok(id)
}

pub struct AttendanceApi {
    client: reqwest::Client,
    base: String,
    token: String,
}

#[cfg(test)]
mod token_tests {
    use super::*;

    #[test]
    fn each_client_generates_a_fresh_bearer_uuid() {
        let tokens = [
            AttendanceApi::random().unwrap().token,
            AttendanceApi::random().unwrap().token,
        ];
        for token in &tokens {
            let uuid = Uuid::parse_str(token.strip_prefix("Bearer ").unwrap()).unwrap();
            assert_eq!(uuid.get_version_num(), 4);
        }
        assert_ne!(tokens[0], tokens[1]);
    }
}

impl AttendanceApi {
    pub fn random() -> Result<Self> {
        // shortcut: these endpoints accept random app tokens; revisit if validation changes.
        Self::new(
            "https://api.ipb.ac.id/v1",
            &format!("Bearer {}", Uuid::new_v4()),
        )
    }

    /// A custom base is useful for local HTTP fixtures. Production uses random.
    pub fn new(base: &str, token: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut response = self
            .client
            .get(format!("{}/{path}", self.base))
            .header("X-IPBAPI-Token", &self.token)
            .header("Accept", "application/json")
            .header("X-IPBM-App", "student")
            .header("X-IPBM-Platform", "android")
            .header("X-IPBM-Version", "3.4.2")
            .query(query)
            .send()
            .await
            .map_err(|_| anyhow!("Attendance API transport failure"))?;
        ensure!(
            response.status() == reqwest::StatusCode::OK,
            "Attendance API returned HTTP {}",
            response.status().as_u16()
        );
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("Attendance API body failure"))?
        {
            ensure!(
                body.len() + chunk.len() <= 1_048_576,
                "Attendance response exceeds limit"
            );
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| anyhow!("Attendance API returned invalid JSON"))
    }

    async fn student_id(&self, nim: &str) -> Result<i64> {
        resolve_student(
            self.get("Mahasiswa/StatusMahasiswa", &[("NIM", nim.to_string())])
                .await?,
            nim,
        )
    }

    async fn snapshot(&self, student_id: i64) -> Result<Snapshot> {
        parse_snapshot(
            self.get(
                "Kehadiran/Mahasiswa/RekapKuliah",
                &[("MahasiswaId", student_id.to_string())],
            )
            .await?,
        )
    }
}

/// Returns false for an existing identical subscription; never resets its baseline.
pub async fn subscribe(
    pool: &PgPool,
    api: &AttendanceApi,
    user: &str,
    chat: &str,
    nim: &str,
) -> Result<bool> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT nim FROM attendance_subscriptions WHERE user_id=$1")
            .bind(user)
            .fetch_optional(pool)
            .await?;
    if existing.as_deref() == Some(nim) {
        return Ok(false);
    }
    let student_id = api.student_id(nim).await?;
    let baseline = api.snapshot(student_id).await?;
    let mut tx = pool.begin().await?;
    let changed: Option<String> = sqlx::query_scalar(
        "INSERT INTO attendance_subscriptions (user_id,chat_id,nim,student_id,generation,baseline)
         VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT(user_id) DO UPDATE
         SET chat_id=EXCLUDED.chat_id,nim=EXCLUDED.nim,student_id=EXCLUDED.student_id,
             generation=EXCLUDED.generation,baseline=EXCLUDED.baseline,last_checked_at=now()
         WHERE attendance_subscriptions.nim != EXCLUDED.nim RETURNING user_id",
    )
    .bind(user)
    .bind(chat)
    .bind(nim)
    .bind(student_id)
    .bind(Uuid::new_v4())
    .bind(Json(baseline))
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM attendance_reminders r USING attendance_subscriptions s
        WHERE r.user_id=s.user_id AND s.user_id=$1 AND r.generation != s.generation",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(changed.is_some())
}

pub async fn unsubscribe(pool: &PgPool, user: &str) -> Result<()> {
    sqlx::query("DELETE FROM attendance_subscriptions WHERE user_id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn handle_command(
    action: ReminderAction,
    user: &str,
    chat: &str,
    pool: &PgPool,
) -> String {
    if user != chat
        || !(chat.ends_with("@c.us") || chat.ends_with("@s.whatsapp.net") || chat.ends_with("@lid"))
    {
        return format!(
            "Pengingat absensi hanya bisa diaktifkan/dimatikan lewat chat pribadi bot. {USAGE}"
        );
    }
    let result = match action {
        ReminderAction::Disable => unsubscribe(pool, user).await.map(|_| "🔕 *Pengingat Absensi Non-Aktif*\n\nPengingat dimatikan dan data absensi tersimpan sudah dihapus.\n_Ketik #remind-absen <NIM> kalau mau aktifkan lagi._".to_string()),
        ReminderAction::Enable(nim) => match AttendanceApi::random() {
            Ok(api) => subscribe(pool, &api, user, chat, &nim).await.map(|created| {
                if created {
                    "✅ *Pengingat Absensi Aktif!*\n\n🕐 Maarbot cek setiap jam. Kalau jumlah tidak hadir/belum tercatat naik, kamu dapat satu pengingat.\n📚 Data awal sudah disimpan; absensi lama tidak dikirim ulang.\n\n_Kamu juga bisa lihat rekap tersimpan lewat API key sendiri. Ketik #apidocs untuk caranya._\n_#remind-absen off untuk berhenti langganan._".to_string()
                } else { "✅ *Pengingat Absensi Sudah Aktif*\n\nSetting NIM dan data pembanding kamu tetap tersimpan.\n_#remind-absen off untuk berhenti langganan._".to_string() }
            }),
            Err(_) => return "❌ Pengingat absensi sedang tidak tersedia.\n_Coba lagi sebentar ya._".to_string(),
        },
    };
    result.unwrap_or_else(|_| "❌ Gagal mengatur pengingat absensi.\nPeriksa NIM kamu atau coba lagi sebentar ya.\n_Setting sebelumnya tetap tersimpan._".to_string())
}

fn reminder_message(changes: &[Change]) -> String {
    let mut text =
        "📝 *[Pengingat Absensi]*\n_Yuk, cek presensi kamu di IPB Mobile!_\n\n".to_string();
    for change in changes {
        // Keep API text from injecting WhatsApp formatting or additional lines.
        let course: String = change
            .course
            .chars()
            .filter(|c| !c.is_control() && !"*_~`".contains(*c))
            .take(120)
            .collect();
        let class: String = change
            .class
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(20)
            .collect();
        text.push_str(&format!(
            "📚 *{}* ({})\n*└─* Tidak hadir/belum tercatat: *{} → {}*\n\n",
            course, class, change.previous, change.current
        ));
    }
    text.push_str("_Angka ini bisa berarti belum isi presensi atau tercatat tidak hadir. Cek dulu ya!_\n\n_#remind-absen off untuk berhenti langganan._");
    text
}

/// Read saved data only; identity always comes from Marbot's key middleware.
pub fn api_router<S: Clone + Send + Sync + 'static>(pool: PgPool) -> axum::Router<S> {
    axum::Router::new()
        .route("/attendance", axum::routing::get(get_attendance))
        .with_state(pool)
}

async fn get_attendance(
    axum::extract::State(pool): axum::extract::State<PgPool>,
    axum::Extension(user): axum::Extension<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, Json};
    let (status, body) = if query.is_some() {
        (
            StatusCode::BAD_REQUEST,
            serde_json::json!({"success":false,"data":null,"error":"Endpoint ini tidak menerima query. Identitas mengikuti API key."}),
        )
    } else {
        let row: Result<
            Option<(
                String,
                sqlx::types::Json<Snapshot>,
                chrono::DateTime<chrono::Utc>,
            )>,
            _,
        > = sqlx::query_as(
            "SELECT nim,baseline,last_checked_at FROM attendance_subscriptions WHERE user_id=$1",
        )
        .bind(&user)
        .fetch_optional(&pool)
        .await;
        match row {
            Ok(Some((nim, sqlx::types::Json(baseline), checked))) => (
                StatusCode::OK,
                serde_json::json!({
                "success":true,"data":{"enabled":true,"nim":nim,"last_checked_at":checked,
                "classes":baseline.into_values().collect::<Vec<_>>()}}),
            ),
            Ok(None) => (
                StatusCode::OK,
                serde_json::json!({"success":true,"data":{
                "enabled":false,"nim":null,"last_checked_at":null,"classes":[]}}),
            ),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"success":false,"data":null,"error":"Gagal membaca pengingat absensi."}),
            ),
        }
    };
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        "Cache-Control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "Vary",
        axum::http::HeaderValue::from_static("Authorization"),
    );
    response
}

/// Each subscriber is locked independently. A failed API request leaves their
/// baseline unchanged, while other subscribers continue to be checked.
pub async fn check_subscriptions(pool: &PgPool, api: &AttendanceApi) -> Result<usize> {
    let users: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM attendance_subscriptions ORDER BY user_id")
            .fetch_all(pool)
            .await?;
    let mut failures = 0;
    for user in users {
        if check_one(pool, api, &user).await.is_err() {
            failures += 1;
        }
    }
    Ok(failures)
}

async fn check_one(pool: &PgPool, api: &AttendanceApi, user: &str) -> Result<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(i64, Uuid, Json<Snapshot>)> = sqlx::query_as(
        "SELECT student_id,generation,baseline FROM attendance_subscriptions WHERE user_id=$1 FOR UPDATE SKIP LOCKED")
        .bind(user).fetch_optional(&mut *tx).await?;
    if let Some((id, generation, Json(mut baseline))) = row {
        let current = api.snapshot(id).await?;
        let changes = compare(&baseline, &current);
        // Preserve missing classes on empty/partial replies; decreases are saved.
        baseline.extend(current);
        sqlx::query("UPDATE attendance_subscriptions SET baseline=$2,last_checked_at=now() WHERE user_id=$1")
            .bind(user).bind(Json(baseline)).execute(&mut *tx).await?;
        if !changes.is_empty() {
            sqlx::query("INSERT INTO attendance_reminders(id,user_id,generation,message) VALUES($1,$2,$3,$4)")
                .bind(Uuid::new_v4()).bind(user).bind(generation).bind(reminder_message(&changes))
                .execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub trait ReminderSender {
    fn send(&self, chat: &str, message: &str) -> impl Future<Output = Result<()>> + Send;
}

pub struct WhatsAppSender;
impl ReminderSender for WhatsAppSender {
    async fn send(&self, chat: &str, message: &str) -> Result<()> {
        match tokio::time::timeout(
            Duration::from_secs(20),
            crate::waha::send_raw_text(chat, message, None, None),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            _ => bail!("Attendance reminder delivery failed or timed out"),
        }
    }
}

/// At most one send attempt per persisted reminder, including across restarts
/// and concurrent workers. A crash after claiming may lose a reminder, but
/// never resends it: WAHA has no established idempotency guarantee here.
pub async fn dispatch_pending(pool: &PgPool, sender: &impl ReminderSender) -> Result<usize> {
    let mut failures = 0;
    loop {
        let mut tx = pool.begin().await?;
        let row: Option<(Uuid, String, String)> = sqlx::query_as(
            "SELECT r.id,s.chat_id,r.message FROM attendance_reminders r
             JOIN attendance_subscriptions s ON r.user_id=s.user_id AND r.generation=s.generation
             WHERE r.attempted_at IS NULL ORDER BY r.created_at LIMIT 1 FOR UPDATE OF r SKIP LOCKED")
            .fetch_optional(&mut *tx).await?;
        let Some((id, chat, message)) = row else {
            tx.commit().await?;
            break;
        };
        sqlx::query("UPDATE attendance_reminders SET attempted_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        if sender.send(&chat, &message).await.is_ok() {
            sqlx::query("UPDATE attendance_reminders SET delivered_at=now() WHERE id=$1")
                .bind(id)
                .execute(pool)
                .await?;
        } else {
            failures += 1;
        }
    }
    Ok(failures)
}
