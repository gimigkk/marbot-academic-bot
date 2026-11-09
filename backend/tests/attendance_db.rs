//! Run against a dedicated disposable database:
//! ATTENDANCE_TEST_DATABASE_URL=postgres://... SQLX_OFFLINE=true \
//! cargo test --test attendance_db -- --ignored
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use marbot::attendance::{self, AttendanceApi, ReminderSender};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct Fixture(Arc<Mutex<(StatusCode, Value)>>);

fn recap(missed: u32) -> Value {
    json!([{"KodeMk":"TEST101","NamaMk":"Test Course","DetailRekap":[{
        "JenisKelas":"K","KelasParalel":"1","JumlahHadir":5,
        "JumlahTidakHadir":missed,"JumlahPertemuan":5+missed
    }]}])
}

async fn student(headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    assert!(!headers.contains_key("authorization"));
    assert_eq!(headers["x-ipbapi-token"], "fixture-token");
    let nim = q.get("NIM").expect("NIM filter is mandatory");
    assert_eq!(q.len(), 1);
    Json(json!({"Items":[{"NIM":nim,"MahasiswaID":42}]}))
}

async fn snapshot(
    State(state): State<Fixture>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    assert!(!headers.contains_key("authorization"));
    assert_eq!(q.get("MahasiswaId").map(String::as_str), Some("42"));
    let (status, body) = state.0.lock().unwrap().clone();
    (status, Json(body))
}

#[derive(Default)]
struct Sender {
    messages: Mutex<Vec<(String, String)>>,
    fail: bool,
}
impl ReminderSender for Sender {
    async fn send(&self, chat: &str, message: &str) -> anyhow::Result<()> {
        self.messages
            .lock()
            .unwrap()
            .push((chat.to_string(), message.to_string()));
        if self.fail {
            anyhow::bail!("simulated uncertain delivery");
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires ATTENDANCE_TEST_DATABASE_URL pointing to a disposable database"]
async fn durable_opt_in_polling_and_single_delivery() {
    let url =
        std::env::var("ATTENDANCE_TEST_DATABASE_URL").expect("dedicated test database required");
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&url)
        .await
        .unwrap();
    // This deliberately fails if tables exist, protecting an existing database.
    sqlx::raw_sql(include_str!(
        "../migrations/20261109120000_attendance_reminders.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let state = Fixture(Arc::new(Mutex::new((StatusCode::OK, recap(1)))));
    let app = Router::new()
        .route("/v1/Mahasiswa/StatusMahasiswa", get(student))
        .route("/v1/Kehadiran/Mahasiswa/RekapKuliah", get(snapshot))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async {
        axum::serve(listener, app).await.unwrap();
    });
    let api = AttendanceApi::new(&base, "fixture-token").unwrap();
    let user = "fixture@c.us";
    assert!(
        attendance::subscribe(&pool, &api, user, user, "G6401260001")
            .await
            .unwrap()
    );
    let sender = Sender::default();
    attendance::dispatch_pending(&pool, &sender).await.unwrap();
    assert!(sender.messages.lock().unwrap().is_empty()); // initial baseline is silent

    *state.0.lock().unwrap() = (StatusCode::OK, recap(2));
    assert!(
        !attendance::subscribe(&pool, &api, user, user, "G6401260001")
            .await
            .unwrap()
    );
    // Enabling twice must not consume the 1 -> 2 increase.
    let (a, b) = tokio::join!(
        attendance::check_subscriptions(&pool, &api),
        attendance::check_subscriptions(&pool, &api)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 0);
    let (a, b) = tokio::join!(
        attendance::dispatch_pending(&pool, &sender),
        attendance::dispatch_pending(&pool, &sender)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 0);
    assert_eq!(sender.messages.lock().unwrap().len(), 1);
    let first = sender.messages.lock().unwrap()[0].clone();
    assert_eq!(first.0, user);
    assert!(first.1.contains("1 → 2"));

    // New pool simulates process restart: persisted state suppresses duplicates.
    let restarted = PgPoolOptions::new().connect(&url).await.unwrap();
    attendance::check_subscriptions(&restarted, &api)
        .await
        .unwrap();
    attendance::dispatch_pending(&restarted, &sender)
        .await
        .unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 1);

    // Errors and empty responses cannot wipe a baseline.
    *state.0.lock().unwrap() = (StatusCode::UNAUTHORIZED, json!({"error":"fixture"}));
    assert_eq!(
        attendance::check_subscriptions(&pool, &api).await.unwrap(),
        1
    );
    *state.0.lock().unwrap() = (StatusCode::OK, json!({"unexpected":"schema"}));
    assert_eq!(
        attendance::check_subscriptions(&pool, &api).await.unwrap(),
        1
    );
    *state.0.lock().unwrap() = (StatusCode::OK, json!([]));
    assert_eq!(
        attendance::check_subscriptions(&pool, &api).await.unwrap(),
        0
    );
    *state.0.lock().unwrap() = (StatusCode::OK, recap(3));
    attendance::check_subscriptions(&pool, &api).await.unwrap();
    attendance::dispatch_pending(&pool, &sender).await.unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
    assert!(sender.messages.lock().unwrap()[1].1.contains("2 → 3"));

    // Correction lowers the baseline; the next increase is a new reminder.
    *state.0.lock().unwrap() = (StatusCode::OK, recap(1));
    attendance::check_subscriptions(&pool, &api).await.unwrap();
    *state.0.lock().unwrap() = (StatusCode::OK, recap(2));
    attendance::check_subscriptions(&pool, &api).await.unwrap();
    let failing = Sender {
        fail: true,
        ..Default::default()
    };
    assert_eq!(
        attendance::dispatch_pending(&pool, &failing).await.unwrap(),
        1
    );
    attendance::dispatch_pending(&restarted, &failing)
        .await
        .unwrap();
    assert_eq!(failing.messages.lock().unwrap().len(), 1); // no retry after uncertain send

    *state.0.lock().unwrap() = (StatusCode::OK, recap(3));
    attendance::check_subscriptions(&pool, &api).await.unwrap();
    // Switching NIM cancels the old pending reminder and creates a silent baseline.
    assert!(
        attendance::subscribe(&pool, &api, user, user, "G6401260002")
            .await
            .unwrap()
    );
    attendance::dispatch_pending(&pool, &sender).await.unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
    *state.0.lock().unwrap() = (StatusCode::OK, recap(4));
    attendance::check_subscriptions(&pool, &api).await.unwrap();
    attendance::unsubscribe(&pool, user).await.unwrap(); // cancels pending, deletes state
    attendance::dispatch_pending(&pool, &sender).await.unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM attendance_reminders")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(
        attendance::subscribe(&pool, &api, user, user, "G6401260001")
            .await
            .unwrap()
    );
    attendance::dispatch_pending(&pool, &sender).await.unwrap();
    assert_eq!(sender.messages.lock().unwrap().len(), 2); // re-enable is a silent baseline

    let reply = attendance::handle_command(
        attendance::ReminderAction::Disable,
        user,
        "group@g.us",
        &pool,
    )
    .await;
    assert!(reply.contains("chat pribadi"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM attendance_subscriptions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1); // group command cannot disable a private subscription

    sqlx::raw_sql(include_str!(
        "../migrations/20261109120000_attendance_reminders.down.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    server.abort();
    restarted.close().await;
    pool.close().await;
}
