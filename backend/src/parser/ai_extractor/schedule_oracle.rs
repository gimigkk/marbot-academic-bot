// backend/src/parser/ai_extractor/schedule_oracle.rs

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct ScheduleData {
    #[serde(rename = "Senin")]
    senin: Vec<CourseSchedule>,
    #[serde(rename = "Selasa")]
    selasa: Vec<CourseSchedule>,
    #[serde(rename = "Rabu")]
    rabu: Vec<CourseSchedule>,
    #[serde(rename = "Kamis")]
    kamis: Vec<CourseSchedule>,
    #[serde(rename = "Jumat")]
    jumat: Vec<CourseSchedule>,
}

#[derive(Debug, Deserialize, Clone)]
struct CourseSchedule {
    course: String,
    parallel: String,
    schedule: String, // e.g., "08:00-09:40"
}

pub struct ScheduleOracle {
    // Map: (course_code, parallel) -> Vec<(Weekday, start_time)>
    schedules: HashMap<(String, String), Vec<(Weekday, String)>>,
}

impl ScheduleOracle {
    /// Load from your JSON file
    pub fn load_from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read schedule file: {}", e))?;
        
        let data: ScheduleData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse schedule JSON: {}", e))?;
        
        let mut schedules: HashMap<(String, String), Vec<(Weekday, String)>> = HashMap::new();
        
        // Process each day
        Self::process_day(&mut schedules, &data.senin, Weekday::Mon);
        Self::process_day(&mut schedules, &data.selasa, Weekday::Tue);
        Self::process_day(&mut schedules, &data.rabu, Weekday::Wed);
        Self::process_day(&mut schedules, &data.kamis, Weekday::Thu);
        Self::process_day(&mut schedules, &data.jumat, Weekday::Fri);
        
        Ok(Self { schedules })
    }
    
    fn process_day(
        schedules: &mut HashMap<(String, String), Vec<(Weekday, String)>>,
        day_schedules: &[CourseSchedule],
        weekday: Weekday,
    ) {
        for schedule in day_schedules {
            // Extract course code (e.g., "KOM120C" from "KOM120C - Pemrograman")
            let course_code = schedule.course
                .split(" - ")
                .next()
                .unwrap_or(&schedule.course)
                .trim()
                .to_string();
            
            // Extract start time (e.g., "08:00" from "08:00-09:40")
            let start_time = schedule.schedule
                .split('-')
                .next()
                .unwrap_or(&schedule.schedule)
                .trim()
                .to_string();
            
            let key = (course_code, schedule.parallel.to_lowercase());
            schedules
                .entry(key)
                .or_insert_with(Vec::new)
                .push((weekday, start_time));
        }
    }
    
    /// NEW: Get next meeting with time (date and start time)
    pub fn get_next_meeting_with_time(
        &self,
        course_name: &str,
        parallel_code: &str,
        from_date: NaiveDate,
    ) -> Option<(NaiveDate, String)> {
        // Try to find matching course by name (fuzzy match)
        let parallel_lower = parallel_code.to_lowercase();
        
        let matching_schedule = self.schedules
            .iter()
            .find(|((code, parallel), _)| {
                parallel == &parallel_lower && 
                Self::course_matches(code, course_name)
            })?;
        
        let schedule_times = matching_schedule.1;
        
        // Find next occurrence
        let current_weekday = from_date.weekday();
        let mut next_meetings = Vec::new();
        
        for (weekday, time) in schedule_times {
            let days_ahead = Self::days_until_weekday(current_weekday, *weekday);
            let next_date = from_date + Duration::days(days_ahead);
            next_meetings.push((next_date, time.clone()));
        }
        
        // Sort by date, then by time
        next_meetings.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1))
        });
        
        next_meetings.into_iter().next()
    }
    
    /// Get next meeting for a course and parallel (date only - backward compatible)
    pub fn get_next_meeting(
        &self,
        course_name: &str,
        parallel_code: &str,
        from_date: NaiveDate,
    ) -> Option<NaiveDate> {
        self.get_next_meeting_with_time(course_name, parallel_code, from_date)
            .map(|(date, _time)| date)
    }
    
    /// Check if course code matches course name
    fn course_matches(course_code: &str, course_name: &str) -> bool {
        let name_lower = course_name.to_lowercase();
        
        // Map course codes to names (based on your data)
        let mapping = [
            ("kom1221", vec!["metode kuantitatif", "metkuan", "mk"]),
            ("kom120d", vec!["matematika komputasi", "matkom", "pengantar matematika"]),
            ("kom120c", vec!["pemrograman", "pemrog"]),
            ("kom120g", vec!["organisasi dan arsitektur komputer", "orkom", "oaak"]),
            ("kom120h", vec!["struktur data", "sd", "strukdat"]),
            ("kom1231", vec!["rekayasa perangkat lunak", "rpl"]),
            ("kom1232", vec!["desain pengalaman pengguna", "ux", "uxd", "dpp"]),
            ("kom1304", vec!["grafika komputer dan visualisasi", "grafkom", "gkv"]),
        ];
        
        let code_lower = course_code.to_lowercase();
        
        for (code, aliases) in &mapping {
            if code_lower.contains(code) {
                for alias in aliases {
                    if name_lower.contains(alias) {
                        return true;
                    }
                }
            }
        }
        
        false
    }
    
    fn days_until_weekday(from: Weekday, to: Weekday) -> i64 {
        let from_num = from.num_days_from_monday();
        let to_num = to.num_days_from_monday();
        
        if to_num > from_num {
            (to_num - from_num) as i64
        } else if to_num < from_num {
            (7 - from_num + to_num) as i64
        } else {
            7 // Same day -> next week
        }
    }
    
    /// Get all schedule info for debugging
    pub fn get_schedule_for_course(
        &self,
        course_name: &str,
        parallel_code: &str,
    ) -> Option<Vec<(Weekday, String)>> {
        let parallel_lower = parallel_code.to_lowercase();
        
        self.schedules
            .iter()
            .find(|((code, parallel), _)| {
                parallel == &parallel_lower && 
                Self::course_matches(code, course_name)
            })
            .map(|(_, schedule)| schedule.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_days_until_weekday() {
        // Monday to Wednesday = 2 days
        assert_eq!(ScheduleOracle::days_until_weekday(Weekday::Mon, Weekday::Wed), 2);
        
        // Friday to Monday = 3 days
        assert_eq!(ScheduleOracle::days_until_weekday(Weekday::Fri, Weekday::Mon), 3);
        
        // Same day = 7 days (next week)
        assert_eq!(ScheduleOracle::days_until_weekday(Weekday::Mon, Weekday::Mon), 7);
    }
    
    #[test]
    fn test_course_matches() {
        assert!(ScheduleOracle::course_matches("KOM120C", "Pemrograman"));
        assert!(ScheduleOracle::course_matches("KOM120C", "pemrog"));
        assert!(ScheduleOracle::course_matches("KOM1231", "RPL"));
        assert!(!ScheduleOracle::course_matches("KOM120C", "Struktur Data"));
    }
}