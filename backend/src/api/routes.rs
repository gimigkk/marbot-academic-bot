use axum::{
    extract::{State, Query},
    http::StatusCode,
    Json,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

const MAX_PARAM_LEN: usize = 100;

#[derive(Deserialize)]
pub struct AssignmentsQuery {
    pub timeframe: Option<String>,
    pub course: Option<String>,
    pub parallel: Option<String>,
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn get_assignments(
    State(state): State<crate::AppState>,
    Query(params): Query<AssignmentsQuery>,
    axum::extract::Extension(user_id): axum::extract::Extension<String>,
) -> impl IntoResponse {
    // Fix #10: Validate query parameter lengths to prevent oversized string attacks
    if params.course.as_deref().map(|s| s.len()).unwrap_or(0) > MAX_PARAM_LEN
        || params.parallel.as_deref().map(|s| s.len()).unwrap_or(0) > MAX_PARAM_LEN
        || params.timeframe.as_deref().map(|s| s.len()).unwrap_or(0) > MAX_PARAM_LEN
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<Vec<crate::models::AssignmentWithCourse>> {
                success: false,
                data: vec![],
                error: Some(format!("Query parameters must not exceed {} characters.", MAX_PARAM_LEN)),
            }),
        ).into_response();
    }

    let pool = &state.pool;

    // Always scope to the authenticated user's data.
    // Users can still filter by course/parallel via query params.
    let mut assignments = match crate::database::crud::get_active_assignments_for_user(pool, &user_id, None).await {
        Ok((a, _)) => a,
        Err(e) => {
            eprintln!("API DB error for user {}: {}", user_id, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<Vec<crate::models::AssignmentWithCourse>> {
                    success: false,
                    data: vec![],
                    error: Some("Internal server error.".to_string()),
                }),
            ).into_response();
        }
    };

    // Filter by timeframe if needed
    // Fix #8: invalid timeframe values (e.g. "all") are treated as no filter, clearly documented
    if let Some(tf) = &params.timeframe {
        let gmt7 = chrono::FixedOffset::east_opt(7 * 3600).unwrap();
        let now = chrono::Utc::now().with_timezone(&gmt7).date_naive();

        match tf.as_str() {
            "today" => {
                assignments.retain(|a| {
                    a.deadline.map(|d| d.with_timezone(&gmt7).date_naive() == now).unwrap_or(false)
                });
            }
            "week" => {
                let week_end = now + chrono::Duration::days(7);
                assignments.retain(|a| {
                    a.deadline.map(|d| {
                        let date = d.with_timezone(&gmt7).date_naive();
                        date >= now && date <= week_end
                    }).unwrap_or(false)
                });
            }
            // "all" or any unknown value = return everything (no filter), which is intentional
            _ => {}
        }
    }

    // Filter by course name/alias
    if let Some(course) = &params.course {
        let c_lower = course.to_lowercase();
        assignments.retain(|a| {
            a.course_name.to_lowercase().contains(&c_lower)
                || a.first_alias.to_lowercase().contains(&c_lower)
        });
    }

    // Filter by parallel class (bypasses #setkelas)
    if let Some(parallel) = &params.parallel {
        let p_lower = parallel.to_lowercase();
        assignments.retain(|a| {
            a.parallel_codes.is_empty()
                || a.parallel_codes.contains(&"all".to_string())
                || a.parallel_codes.iter().any(|c| c.to_lowercase() == p_lower)
        });
    }

    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: assignments,
            error: None,
        }),
    ).into_response()
}
