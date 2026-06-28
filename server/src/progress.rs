use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type ProgressMap = Arc<Mutex<HashMap<String, f32>>>;

pub fn new_progress_map() -> ProgressMap {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn set(map: &ProgressMap, id: &str, pct: f32) {
    if let Ok(mut m) = map.lock() {
        m.insert(id.to_string(), pct.clamp(0.0, 1.0));
    }
}

pub fn remove(map: &ProgressMap, id: &str) {
    if let Ok(mut m) = map.lock() {
        m.remove(id);
    }
}

pub fn get(map: &ProgressMap, id: &str) -> Option<f32> {
    map.lock().ok()?.get(id).copied()
}
