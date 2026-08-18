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
    schedule: String, 
}

pub struct ScheduleOracle {
    schedules: HashMap<(String, String), Vec<(Weekday, String)>>,
}

impl ScheduleOracle {
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
            let course_name = schedule.course
                .split(" - ")
                .last()
                .unwrap_or(&schedule.course)
                .trim()
                .to_lowercase();
          
            let start_time = schedule.schedule
                .split('-')
                .next()
                .unwrap_or(&schedule.schedule)
                .trim()
                .to_string();
            
            let key = (course_name, schedule.parallel.to_lowercase());
            schedules
                .entry(key)
                .or_insert_with(Vec::new)
                .push((weekday, start_time));
        }
    }
   
    pub fn get_next_meeting_with_time(
        &self,
        course_name: &str,
        parallel_code: &str,
        from_date: NaiveDate,
    ) -> Option<(NaiveDate, String)> {
        let parallel_lower = parallel_code.to_lowercase();
        
        let matching_schedule = self.schedules
            .iter()
            .find(|((stored_name, parallel), _)| {
                parallel == &parallel_lower && 
                Self::course_matches(stored_name, course_name)
            })?;
        
        let schedule_times = matching_schedule.1;
      
        let current_weekday = from_date.weekday();
        let mut next_meetings = Vec::new();
        
        for (weekday, time) in schedule_times {
            let days_ahead = Self::days_until_weekday(current_weekday, *weekday);
            let next_date = from_date + Duration::days(days_ahead);
            next_meetings.push((next_date, time.clone()));
        }
      
        next_meetings.sort_by(|a, b| {
            a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1))
        });
        
        next_meetings.into_iter().next()
    }
    
    pub fn get_next_meeting(
        &self,
        course_name: &str,
        parallel_code: &str,
        from_date: NaiveDate,
    ) -> Option<NaiveDate> {
        self.get_next_meeting_with_time(course_name, parallel_code, from_date)
            .map(|(date, _time)| date)
    }
    
    fn course_matches(stored_course: &str, query_course: &str) -> bool {
        let stored_lower = stored_course.to_lowercase();
        let query_lower = query_course.to_lowercase();
        
        if stored_lower == query_lower || stored_lower.contains(&query_lower) || query_lower.contains(&stored_lower) {
            return true;
        }

        let mapping = [
            ("analisis algoritme", vec!["analisis algoritme", "analisis algoritma", "analgor", "aa", "algoritme", "algoritma"]),
            ("komunikasi data dan jaringan komputer", vec!["komunikasi data dan jaringan komputer", "komunikasi data", "komdat", "jarkom", "kj", "kdj", "jaringan komputer"]),
            ("kecerdasan buatan", vec!["kecerdasan buatan", "ai", "kb"]),
            ("keamanan informasi", vec!["keamanan informasi", "keamanan info", "kemin", "ki", "kaminfo"]),
            ("sistem operasi", vec!["sistem operasi", "so", "os"]),
            ("sistem informasi", vec!["sistem informasi", "si", "is"]),
        ];
        
        for (canonical, aliases) in &mapping {
            if stored_lower.contains(canonical) {
                for alias in aliases {
                    if query_lower == *alias || query_lower.contains(alias) {
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
            7 
        }
    }
    
    pub fn get_schedule_for_course(
        &self,
        course_name: &str,
        parallel_code: &str,
    ) -> Option<Vec<(Weekday, String)>> {
        let parallel_lower = parallel_code.to_lowercase();
        
        self.schedules
            .iter()
            .find(|((stored_name, parallel), _)| {
                parallel == &parallel_lower && 
                Self::course_matches(stored_name, course_name)
            })
            .map(|(_, schedule)| schedule.clone())
    }
}
